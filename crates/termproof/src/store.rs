//! Canonical artifact storage.
//!
//! Guarantees:
//! - **Path traversal guard**: recipe/renderer names are sanitized; resulting
//!   paths are validated to remain under `base_dir`.
//! - **Atomic writes**: JSON and report files are written via temp-file +
//!   rename, so concurrent runs never observe a torn file.
//! - **Race-safe dirs**: each run gets a unique timestamp + pid + random
//!   suffix, so parallel invocations never collide.
//! - **Partial-run handling**: `write_result_files` succeeds even when the
//!   run directory already exists and may contain partial evidence.

use crate::result::RunResult;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Sanitize a user-controlled path component.
///
/// Mirrors Python's `safe_name = "".join(ch if ch.isalnum() or ch in "-_" else "-")`.
pub fn sanitize_component(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

/// Build a new run directory path without creating it.
///
/// Format: `<base_dir>/<YYYYmmdd-HHMMSS-micros>-<safe_recipe>-<safe_renderer>`
/// The microsecond field ensures ordering even for sub-second parallel runs,
/// while a pid suffix guarantees uniqueness across processes.
pub fn new_run_dir(base_dir: &Path, recipe_name: &str, renderer: &str) -> PathBuf {
    let safe_recipe = sanitize_component(recipe_name);
    let safe_renderer = sanitize_component(renderer);
    let timestamp = timestamp_micros();
    let pid = std::process::id();
    // Small random suffix from process time to avoid collisions within same microsecond.
    let extra = extra_suffix();
    base_dir.join(format!(
        "{timestamp}-{safe_recipe}-{safe_renderer}-{pid}-{extra}"
    ))
}

fn timestamp_micros() -> String {
    let now = chrono::Local::now();
    // Use chrono formatting then append micros.
    let base = now.format("%Y%m%d-%H%M%S").to_string();
    let micros = now.timestamp_subsec_micros() % 1_000_000;
    format!("{base}-{micros:06}")
}

fn extra_suffix() -> String {
    // Use lower 16 bits of nanos for extra entropy.
    let nanos = chrono::Local::now().timestamp_subsec_nanos();
    format!("{:04x}", nanos & 0xFFFF)
}

/// Ensure `candidate` is within `base` (path traversal guard).
///
/// Returns an error if the candidate escapes the base directory.
/// Normalizes via lexical comparison; does not require the path to exist yet.
pub fn ensure_within_base(base: &Path, candidate: &Path) -> Result<(), String> {
    let base_canonical = normalize_lexical(base);
    let cand_canonical = normalize_lexical(candidate);
    if cand_canonical.starts_with(&base_canonical) {
        Ok(())
    } else {
        Err(format!(
            "path traversal detected: {} escapes base {}",
            candidate.display(),
            base.display()
        ))
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Atomically write `content` to `dest` (temp file + rename).
///
/// The temp file is created in the same directory as `dest` so the rename
/// is atomic on the same filesystem.
pub fn atomic_write(dest: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    tmp.flush()?;
    // Persist by renaming; `persist` handles cross-platform atomic rename.
    match tmp.persist(dest) {
        Ok(_) => Ok(()),
        Err(e) => {
            // On Windows persist may fail if dest exists; fallback to copy.
            let _ = std::fs::remove_file(dest);
            e.file.persist(dest).map(|_| ()).map_err(|e2| e2.error)
        }
    }
}

/// Atomically write `content` as UTF-8 text.
pub fn atomic_write_text(dest: &Path, content: &str) -> std::io::Result<()> {
    atomic_write(dest, content.as_bytes())
}

/// Write canonical `result.json` and `report.md` into `run_dir` atomically.
///
/// The report is generated via the evidence crate's pipeline; here we accept
/// a pre-rendered report string so core does not depend on evidence.
pub fn write_result_files(
    run_dir: &Path,
    result: &RunResult,
    report_markdown: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(run_dir)?;
    let result_path = run_dir.join("result.json");
    atomic_write_text(&result_path, &result.to_json_pretty())?;
    let report_path = run_dir.join("report.md");
    atomic_write_text(&report_path, report_markdown)?;
    Ok(())
}

/// Write `latest-report.md` (or `.xml`) at `out_dir` atomically, combining
/// all results.  Kept minimal here; full aggregate is in `termproof-evidence`.
pub fn write_latest_report(out_dir: &Path, report: &str, extension: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join(format!("latest-report{extension}"));
    atomic_write_text(&path, report)
}

/// Validate an artifact path value: must not contain `..` and must be within
/// the run directory if it is a relative path.
pub fn validate_artifact_path(run_dir: &Path, value: &str) -> Result<(), String> {
    let p = Path::new(value);
    if value.contains("..") {
        return Err(format!("artifact path contains '..': {value}"));
    }
    if p.is_relative() {
        let joined = run_dir.join(p);
        ensure_within_base(run_dir, &joined)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_unsafe_chars() {
        assert_eq!(sanitize_component("hello world!"), "hello-world-");
        assert_eq!(sanitize_component(""), "default");
        assert_eq!(sanitize_component("a/b\\c"), "a-b-c");
    }

    #[test]
    fn new_run_dir_is_within_base() {
        let base = Path::new("/tmp/termproof-runs");
        let dir = new_run_dir(base, "my recipe", "default");
        assert!(ensure_within_base(base, &dir).is_ok());
        assert!(dir.to_string_lossy().contains("my-recipe"));
    }

    #[test]
    fn traversal_is_rejected() {
        let base = Path::new("/tmp/base");
        let evil = Path::new("/tmp/base/../etc/passwd");
        assert!(ensure_within_base(base, evil).is_err());
    }

    #[test]
    fn atomic_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.txt");
        atomic_write_text(&dest, "hello\n").unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello\n");
        // Overwrite atomically
        atomic_write_text(&dest, "world\n").unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "world\n");
    }
}
