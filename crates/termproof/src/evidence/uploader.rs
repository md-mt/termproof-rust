//! Publishing evidence somewhere a reviewer can open it.
//!
//! A screenshot on the machine that produced it helps nobody. This module owns
//! the [`ArtifactUploader`] seam and the fallback composition; *where* things
//! go is the caller's business — an object store, a paste service, a CDN, a
//! directory served over HTTP.
//!
//! **Uploads are best-effort by design.** A failure yields `None`, not an
//! error, so a flaky store never fails a run that otherwise produced its
//! evidence. Callers that genuinely require a shareable URL check for one and
//! decide; the default is that missing media degrades the report rather than
//! the verdict.

use std::process::Command;
use std::time::Duration;

use crate::terminal::proc::combined_output;
use crate::terminal::proc::run_with_timeout;

const MAX_ERROR_LENGTH: usize = 500;
const TOOL_TIMEOUT: Duration = Duration::from_secs(120);

/// Runs an external tool with `args` and returns combined stdout+stderr, or an
/// error string. Injected in tests.
pub type ToolRunner = Box<dyn Fn(&[String]) -> Result<String, String> + Send + Sync>;

/// Something that can upload a local artifact and yield a shareable URL.
pub trait ArtifactUploader {
    /// Upload `path`; return its URL, or `None` on failure (see [`last_error`]).
    fn upload(&mut self, path: &str) -> Option<String>;
    /// The last failure message, if the most recent `upload` returned `None`.
    fn last_error(&self) -> Option<&str>;
}

/// Trim `value` and cap it at 500 characters, appending `...` if cut.
///
/// Tool output is arbitrary and can be megabytes; an error message that long
/// is not an error message. Public so product-side [`ArtifactUploader`]
/// implementations clip their failures the same way.
pub fn clip(value: &str) -> String {
    let compact = value.trim();
    if compact.len() <= MAX_ERROR_LENGTH {
        return compact.to_string();
    }
    // Cut on a char boundary: slicing at a fixed byte offset can panic if it
    // lands inside a multi-byte UTF-8 character (tool stdout/stderr is
    // arbitrary bytes, not guaranteed ASCII).
    let cut = compact
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= MAX_ERROR_LENGTH - 3)
        .last()
        .unwrap_or(0);
    format!("{}...", &compact[..cut])
}

/// Run an external upload tool and return its combined output.
///
/// Falls back to `/usr/local/bin/<executable>` if the bare name is not on
/// PATH. Public so product-side [`ArtifactUploader`] implementations get the
/// same timeout and error clipping.
pub fn run_tool(executable: &str, args: &[String]) -> Result<String, String> {
    let attempt = |exe: &str| -> std::io::Result<std::process::Output> {
        let mut cmd = Command::new(exe);
        cmd.args(args);
        run_with_timeout(cmd, TOOL_TIMEOUT)
    };
    let output = match attempt(executable) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            attempt(&format!("/usr/local/bin/{}", executable)).map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let combined = combined_output(&output);
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        return Err(format!(
            "{} exited {}: {}",
            executable,
            code,
            clip(&combined)
        ));
    }
    Ok(combined)
}

/// Tries uploaders in order and returns the first URL.
pub struct FallbackUploader {
    uploaders: Vec<Box<dyn ArtifactUploader>>,
    last_error: Option<String>,
    /// Errors from uploaders that failed *before* a later one succeeded.
    ///
    /// A degraded upload still returns a URL, so without this the primary's
    /// failure is discarded and a run that quietly stopped using its preferred
    /// store reads as completely clean. Deduped by message: one entry per
    /// distinct failure mode, not one per artifact.
    degraded: Vec<String>,
}

impl FallbackUploader {
    /// Compose `uploaders` into a chain, tried in the order given.
    pub fn new(uploaders: Vec<Box<dyn ArtifactUploader>>) -> Self {
        FallbackUploader {
            uploaders,
            last_error: None,
            degraded: Vec::new(),
        }
    }

    /// Distinct failure modes that were survived by falling back.
    pub fn degraded(&self) -> &[String] {
        &self.degraded
    }

    fn record_degraded(&mut self, message: String) {
        if self.degraded.contains(&message) {
            return;
        }
        // Once per distinct message: a recipe uploads a video plus a screenshot
        // per step, so per-artifact logging would repeat this dozens of times.
        eprintln!("Media upload fell back to a secondary store: {}", message);
        self.degraded.push(message);
    }
}

impl ArtifactUploader for FallbackUploader {
    fn upload(&mut self, path: &str) -> Option<String> {
        self.last_error = None;
        let mut errors: Vec<String> = Vec::new();
        let mut uploaded = None;
        for uploader in self.uploaders.iter_mut() {
            if let Some(url) = uploader.upload(path) {
                uploaded = Some(url);
                break;
            }
            if let Some(e) = uploader.last_error() {
                if !e.is_empty() {
                    errors.push(e.to_string());
                }
            }
        }
        if let Some(url) = uploaded {
            if !errors.is_empty() {
                self.record_degraded(errors.join("; "));
            }
            return Some(url);
        }
        self.last_error = Some(if errors.is_empty() {
            "all uploaders returned no URL".to_string()
        } else {
            errors.join("; ")
        });
        None
    }

    fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_returns_first_success() {
        struct Fail;
        impl ArtifactUploader for Fail {
            fn upload(&mut self, _: &str) -> Option<String> {
                None
            }
            fn last_error(&self) -> Option<&str> {
                Some("nope")
            }
        }
        struct Ok_;
        impl ArtifactUploader for Ok_ {
            fn upload(&mut self, _: &str) -> Option<String> {
                Some("https://x/y".to_string())
            }
            fn last_error(&self) -> Option<&str> {
                None
            }
        }
        let mut fb = FallbackUploader::new(vec![Box::new(Fail), Box::new(Ok_)]);
        assert_eq!(fb.upload("/tmp/a").as_deref(), Some("https://x/y"));
        // last_error stays clear on success — existing callers depend on it.
        assert!(fb.last_error().is_none());
    }

    #[test]
    fn fallback_records_the_primarys_failure_even_when_a_later_one_succeeds() {
        struct Fail(&'static str);
        impl ArtifactUploader for Fail {
            fn upload(&mut self, _: &str) -> Option<String> {
                None
            }
            fn last_error(&self) -> Option<&str> {
                Some(self.0)
            }
        }
        struct Ok_;
        impl ArtifactUploader for Ok_ {
            fn upload(&mut self, _: &str) -> Option<String> {
                Some("https://x/y".to_string())
            }
            fn last_error(&self) -> Option<&str> {
                None
            }
        }
        let mut fb =
            FallbackUploader::new(vec![Box::new(Fail("CAT minting failed")), Box::new(Ok_)]);

        // Every artifact in a run hits the same broken primary; the failure is
        // recorded once, not once per upload.
        for _ in 0..10 {
            assert!(fb.upload("/tmp/a").is_some());
        }
        assert_eq!(fb.degraded(), ["CAT minting failed"]);
    }

    #[test]
    fn fallback_records_distinct_failure_modes_separately() {
        struct Flaky {
            calls: usize,
        }
        impl ArtifactUploader for Flaky {
            fn upload(&mut self, _: &str) -> Option<String> {
                self.calls += 1;
                None
            }
            fn last_error(&self) -> Option<&str> {
                if self.calls > 1 {
                    Some("denied")
                } else {
                    Some("timeout")
                }
            }
        }
        struct Ok_;
        impl ArtifactUploader for Ok_ {
            fn upload(&mut self, _: &str) -> Option<String> {
                Some("https://x/y".to_string())
            }
            fn last_error(&self) -> Option<&str> {
                None
            }
        }
        let mut fb = FallbackUploader::new(vec![Box::new(Flaky { calls: 0 }), Box::new(Ok_)]);
        fb.upload("/tmp/a");
        fb.upload("/tmp/b");
        assert_eq!(fb.degraded(), ["timeout", "denied"]);
    }

    #[test]
    fn fallback_records_nothing_when_the_primary_succeeds() {
        struct Ok_;
        impl ArtifactUploader for Ok_ {
            fn upload(&mut self, _: &str) -> Option<String> {
                Some("https://x/y".to_string())
            }
            fn last_error(&self) -> Option<&str> {
                None
            }
        }
        let mut fb = FallbackUploader::new(vec![Box::new(Ok_)]);
        fb.upload("/tmp/a");
        assert!(fb.degraded().is_empty());
    }

    #[test]
    fn fallback_aggregates_errors() {
        struct Fail(&'static str);
        impl ArtifactUploader for Fail {
            fn upload(&mut self, _: &str) -> Option<String> {
                None
            }
            fn last_error(&self) -> Option<&str> {
                Some(self.0)
            }
        }
        let mut fb = FallbackUploader::new(vec![Box::new(Fail("a")), Box::new(Fail("b"))]);
        assert!(fb.upload("/tmp/a").is_none());
        assert_eq!(fb.last_error(), Some("a; b"));
    }

    #[test]
    fn clip_does_not_panic_on_multibyte_boundary() {
        // A run of 3-byte UTF-8 characters straddling the MAX_ERROR_LENGTH-3
        // cut point used to panic ("byte index is not a char boundary").
        let long = "€".repeat(MAX_ERROR_LENGTH);
        let clipped = clip(&long);
        assert!(clipped.ends_with("..."));
        assert!(clipped.is_char_boundary(clipped.len()));
    }
}
