//! `pathlib.PurePosixPath`'s lexical joining and rendering.
//!
//! Assertion path resolution is specified against pathlib, not against
//! `std::path::Path`, and the two disagree in ways a corpus notices:
//!
//! | Input | pathlib | `std::path::Path` |
//! |---|---|---|
//! | `Path("")` | `.` | `""` |
//! | `Path("/a")/Path("")` | `/a` | `/a/` |
//! | `Path("a//b")` | `a/b` | `a//b` |
//! | `Path("./a")` | `a` | `./a` |
//!
//! Both keep `..`: neither collapses `sub/../x` to `x`, because the kernel has
//! to resolve that against the real tree and `sub` may not exist. Spec 003
//! FR-011 depends on all five rows.
//!
//! POSIX only. A Windows drive letter or a UNC path is not modelled, and
//! neither is the leading-double-slash case POSIX leaves implementation
//! defined.

use std::fmt;

/// A lexically normalised POSIX path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyPath {
    absolute: bool,
    parts: Vec<String>,
}

impl PyPath {
    /// Parse a path string the way `PurePosixPath(s)` does.
    ///
    /// Empty segments and `.` are dropped — that is what collapses `a//b` and
    /// `./a`. `..` is kept.
    pub fn parse(path: &str) -> Self {
        Self {
            absolute: path.starts_with('/'),
            parts: path
                .split('/')
                .filter(|part| !part.is_empty() && *part != ".")
                .map(str::to_string)
                .collect(),
        }
    }

    /// Whether the path starts at the root.
    pub fn is_absolute(&self) -> bool {
        self.absolute
    }

    /// Join, the way `self / other` does: an absolute `other` replaces `self`
    /// entirely rather than being appended to it.
    pub fn join(&self, other: &Self) -> Self {
        if other.absolute {
            return other.clone();
        }
        let mut parts = self.parts.clone();
        parts.extend(other.parts.iter().cloned());
        Self {
            absolute: self.absolute,
            parts,
        }
    }
}

impl fmt::Display for PyPath {
    /// Render the way `str(path)` does.
    ///
    /// A path with no parts is `/` when absolute and `.` when not — pathlib has
    /// no empty path, which is why `file_exists` with `value: ""` tests the
    /// working directory rather than a file named nothing.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.parts.is_empty() {
            return f.write_str(if self.absolute { "/" } else { "." });
        }
        if self.absolute {
            f.write_str("/")?;
        }
        f.write_str(&self.parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(path: &str) -> String {
        PyPath::parse(path).to_string()
    }

    fn joined(base: &str, other: &str) -> String {
        PyPath::parse(base).join(&PyPath::parse(other)).to_string()
    }

    #[test]
    fn parsing_drops_dot_segments_and_duplicate_separators() {
        assert_eq!(rendered("./a"), "a");
        assert_eq!(rendered("a//b"), "a/b");
        assert_eq!(rendered("a/./b"), "a/b");
        assert_eq!(rendered("/tmp//fx/"), "/tmp/fx");
    }

    /// The row FR-011 turns on: `..` survives, so the path is stat-ed with it.
    #[test]
    fn dotdot_is_never_collapsed() {
        assert_eq!(rendered("sub/../a"), "sub/../a");
        assert_eq!(joined("/tmp/fx", "sub/../a"), "/tmp/fx/sub/../a");
        assert_eq!(joined("/tmp/fx", ".."), "/tmp/fx/..");
    }

    #[test]
    fn there_is_no_empty_path() {
        assert_eq!(rendered(""), ".");
        assert_eq!(rendered("/"), "/");
        assert_eq!(joined("/tmp/fx", ""), "/tmp/fx");
        assert_eq!(joined("", ""), ".");
        assert_eq!(joined(".", "a.txt"), "a.txt");
    }

    #[test]
    fn an_absolute_other_wins() {
        assert_eq!(joined("/tmp/fx", "/etc/passwd"), "/etc/passwd");
        assert_eq!(joined("relative", "/etc"), "/etc");
    }

    #[test]
    fn a_relative_base_stays_relative() {
        assert_eq!(joined("work", "a.txt"), "work/a.txt");
        assert!(!PyPath::parse("work").is_absolute());
        assert!(PyPath::parse("/work").is_absolute());
    }
}
