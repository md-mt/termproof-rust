//! Provenance for the binary under test.
//!
//! A verification report is only as useful as its answer to "which build was
//! this?". [`BuildInfo`] records where the binary came from — an installed
//! build, a source build tied to a change, or a package — so a result can be
//! traced to an exact artifact months later.
//!
//! Deliberately incurious about *what* the binary is: version discovery is
//! `<binary> --version`, and the caller supplies the path.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use termproof_terminal::proc::run_with_timeout;

/// Best-effort `<binary> --version`; `"unknown"` on any failure.
fn probe_version(binary_path: &str) -> String {
    if binary_path.is_empty() {
        return "unknown".to_string();
    }
    let mut cmd = Command::new(binary_path);
    cmd.arg("--version");
    match run_with_timeout(cmd, Duration::from_secs(10)) {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let version = if !stdout.is_empty() { stdout } else { stderr };
            if version.is_empty() {
                "unknown".to_string()
            } else {
                version
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

/// Where the binary under test came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    /// One of [`MODE_INSTALLED`], [`MODE_SOURCE`], [`MODE_PACKAGE`].
    pub mode: String,
    /// Absolute path to the binary.
    pub binary_path: String,
    /// Whatever `<binary> --version` printed, or `"unknown"`.
    pub version: String,
    /// Code-review or change identifier this build came from, if any.
    pub change_id: Option<String>,
    /// VCS commit hash this build came from, if any.
    pub commit_hash: Option<String>,
    /// Build-system target that produced it, if any.
    pub build_target: Option<String>,
    /// When this record was made, not when the binary was built.
    pub build_timestamp: Option<String>,
}

/// A binary already present on the host.
pub const MODE_INSTALLED: &str = "installed";
/// A binary built from a checkout, tied to a change or commit.
pub const MODE_SOURCE: &str = "source";
/// A binary from a distribution package.
pub const MODE_PACKAGE: &str = "package";

impl BuildInfo {
    /// Whether this record is complete enough to trust.
    ///
    /// A source build must name both a target and a change or commit — a
    /// binary you cannot trace back is not provenance, it is a path. An
    /// installed or packaged binary needs only to exist.
    pub fn verify_provenance(&self) -> bool {
        match self.mode.as_str() {
            MODE_SOURCE => {
                self.build_target.is_some()
                    && (self.change_id.is_some() || self.commit_hash.is_some())
                    && !self.binary_path.is_empty()
                    && Path::new(&self.binary_path).exists()
            }
            MODE_INSTALLED | MODE_PACKAGE => {
                !self.binary_path.is_empty() && Path::new(&self.binary_path).exists()
            }
            _ => false,
        }
    }

    /// Describe an already-installed binary, probing its version.
    pub fn from_installed(binary_path: &str) -> Self {
        BuildInfo {
            mode: MODE_INSTALLED.to_string(),
            binary_path: binary_path.to_string(),
            version: probe_version(binary_path),
            change_id: None,
            commit_hash: None,
            build_target: None,
            build_timestamp: Some(termproof_terminal::proc::timestamp()),
        }
    }

    /// Describe a binary built from a checkout, probing its version.
    pub fn from_source_build(
        build_target: &str,
        change_id: Option<String>,
        commit_hash: Option<String>,
        binary_path: &str,
    ) -> Self {
        let version = if binary_path.is_empty() {
            "unknown".to_string()
        } else {
            probe_version(binary_path)
        };
        BuildInfo {
            mode: MODE_SOURCE.to_string(),
            binary_path: binary_path.to_string(),
            version,
            change_id,
            commit_hash,
            build_target: Some(build_target.to_string()),
            build_timestamp: Some(termproof_terminal::proc::timestamp()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_missing_binary_unverified() {
        let bi = BuildInfo {
            mode: MODE_INSTALLED.to_string(),
            binary_path: "/nonexistent/binary".to_string(),
            version: "unknown".to_string(),
            change_id: None,
            commit_hash: None,
            build_target: None,
            build_timestamp: None,
        };
        assert!(!bi.verify_provenance());
    }

    #[test]
    fn source_requires_target_and_ref() {
        let bi = BuildInfo::from_source_build("//t:x", None, None, "");
        assert!(!bi.verify_provenance());
    }

    #[test]
    fn unknown_mode_unverified() {
        let bi = BuildInfo {
            mode: "weird".to_string(),
            binary_path: String::new(),
            version: "unknown".to_string(),
            change_id: None,
            commit_hash: None,
            build_target: None,
            build_timestamp: None,
        };
        assert!(!bi.verify_provenance());
    }
}
