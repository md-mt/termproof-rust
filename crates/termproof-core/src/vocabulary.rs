//! Screen predicates for asserting on terminal output.
//!
//! Every suite that drives a TUI ends up writing "did this fail?" by hand, and
//! ends up writing it several different ways. This module is what those
//! converge on: a configurable failure detector plus a handful of marker
//! helpers, with the case-sensitivity and word-boundary decisions made once and
//! written down.

use std::sync::OnceLock;

use regex::Regex;

/// Matches a tool-result line reporting its own error, e.g.
/// `write_file write_file └ error: permission denied`.
///
/// The `└` prefix is the common rendering for a nested tool result across
/// agent TUIs; adjust the pattern if your target differs.
fn tool_result_error_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)└\s*(?:error|exception):").expect("valid regex"))
}

/// Drop lines where a *tool* reported an error, not the session.
///
/// A blanket "no error anywhere on screen" assertion scans the whole
/// transcript, so a recipe can be failed by a tool it never asked for — which
/// tools an agent reaches for is not something a recipe controls. Two unrelated
/// recipes failed this way on the same run before this existed.
///
/// An error anywhere outside a tool-result line survives, so a genuine failure
/// still fails.
pub fn strip_tool_result_errors(screen: &str) -> String {
    screen
        .lines()
        .filter(|line| !tool_result_error_re().is_match(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Verbs that indicate failure when they begin a line.
///
/// Anchoring matters. `error:` appearing mid-sentence in an agent's prose is
/// not a failure; `error:` at the start of a line is. Callers that need the
/// looser substring behaviour ask for it through
/// [`FailureDetector::anywhere`] — and should read the warning there first.
pub const DEFAULT_LINE_VERBS: [&str; 5] =
    ["traceback", "exception", "error:", "failed to", "panic"];

/// The markers almost every hand-rolled failure check starts with.
///
/// In the suite this was extracted from, twelve independently written
/// `has_failure` functions all began with these four and then diverged.
/// Sharing the core means a new marker is added once.
pub const CORE_FAILURE_MARKERS: [&str; 4] = ["traceback", "exception", "error:", "failed to"];

/// Whether a screen shows failure.
///
/// A builder rather than a function because suites genuinely need different
/// semantics, and unifying them silently changes verdicts. Three matching
/// modes:
///
/// | mode | matches | use when |
/// |---|---|---|
/// | [`line_verbs`](Self::line_verbs) | `^\s*verb` | the default — prose containing "error:" is not a failure |
/// | [`words`](Self::words) | `\bword\b` anywhere | "panic" should match but "panicking" should not |
/// | [`anywhere`](Self::anywhere) | plain substring | you want every occurrence, wherever it is |
///
/// **Choose `anywhere` deliberately.** It matches startup banners and prose.
/// A suite using it for `"failed to"` had every recipe fail on a host that
/// printed `Warning: Failed to mount ...` at launch — a real run scored 2/15
/// for a reason that had nothing to do with the application under test.
#[derive(Debug, Clone)]
pub struct FailureDetector {
    line_verbs: &'static [&'static str],
    words: &'static [&'static str],
    anywhere: &'static [&'static str],
    extra: &'static [&'static str],
    ignore_tool_result_errors: bool,
}

impl FailureDetector {
    /// A line-anchored detector over [`DEFAULT_LINE_VERBS`], and nothing else.
    pub const fn new() -> Self {
        FailureDetector {
            line_verbs: &DEFAULT_LINE_VERBS,
            words: &[],
            anywhere: &[],
            extra: &[],
            ignore_tool_result_errors: false,
        }
    }

    /// A substring detector over [`CORE_FAILURE_MARKERS`], with no line
    /// anchoring at all.
    ///
    /// Replace that default set with [`Self::anywhere`], or keep it and add to
    /// it with [`Self::extra`]. See the warning on [`FailureDetector`] before
    /// choosing this over [`Self::new`].
    pub const fn markers_only() -> Self {
        FailureDetector {
            line_verbs: &[],
            words: &[],
            anywhere: &CORE_FAILURE_MARKERS,
            extra: &[],
            ignore_tool_result_errors: false,
        }
    }

    /// Replace the line-anchored verb set.
    pub const fn line_verbs(mut self, verbs: &'static [&'static str]) -> Self {
        self.line_verbs = verbs;
        self
    }

    /// Substrings matched anywhere, but only at word boundaries.
    ///
    /// Maps the `\b(?:exception|traceback|panic)\b` shape some recipes used:
    /// looser than line-anchored, stricter than a plain substring, so
    /// "exceptional" is not a hit.
    pub const fn words(mut self, words: &'static [&'static str]) -> Self {
        self.words = words;
        self
    }

    /// Replace the case-insensitive substrings matched anywhere on the screen.
    pub const fn anywhere(mut self, markers: &'static [&'static str]) -> Self {
        self.anywhere = markers;
        self
    }

    /// Additional substrings on top of [`Self::anywhere`], so a recipe can keep
    /// the shared core and add only what is peculiar to it.
    pub const fn extra(mut self, markers: &'static [&'static str]) -> Self {
        self.extra = markers;
        self
    }

    /// Drop `└ error:` tool-result lines before matching.
    ///
    /// A tool the recipe never asked for failing in the transcript is not that
    /// recipe's concern.
    pub const fn ignoring_tool_result_errors(mut self) -> Self {
        self.ignore_tool_result_errors = true;
        self
    }

    /// Whether `screen` shows failure under this detector's configuration.
    pub fn detects(&self, screen: &str) -> bool {
        let owned;
        let subject = if self.ignore_tool_result_errors {
            owned = strip_tool_result_errors(screen);
            owned.as_str()
        } else {
            screen
        };
        let lowered = subject.to_lowercase();
        if self
            .anywhere
            .iter()
            .chain(self.extra)
            .any(|m| lowered.contains(&m.to_lowercase()))
        {
            return true;
        }
        if self
            .words
            .iter()
            .any(|w| contains_word(&lowered, &w.to_lowercase()))
        {
            return true;
        }
        lowered.lines().any(|line| {
            let trimmed = line.trim_start();
            self.line_verbs
                .iter()
                .any(|verb| starts_with_word(trimmed, &verb.to_lowercase()))
        })
    }
}

impl Default for FailureDetector {
    fn default() -> Self {
        FailureDetector::new()
    }
}

/// Whether `line` begins with `verb` at a word boundary.
///
/// The boundary matters: the regexes this replaced spelled `traceback\b`, so
/// a line beginning "tracebacks are useful" was not a failure. A bare
/// `starts_with` would call it one.
fn starts_with_word(line: &str, verb: &str) -> bool {
    let Some(rest) = line.strip_prefix(verb) else {
        return false;
    };
    // A verb ending in punctuation (`error:`) carries its own boundary.
    if !verb.ends_with(|c: char| c.is_alphanumeric()) {
        return true;
    }
    !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_')
}

/// Whether `haystack` contains `word` delimited by non-word characters.
fn contains_word(haystack: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    haystack.match_indices(word).any(|(i, _)| {
        let before_ok = haystack[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
        let after_ok = haystack[i + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_word(c));
        before_ok && after_ok
    })
}

/// How many of `markers` appear in `haystack`, case-insensitively.
///
/// Both sides are lowered. Lowering only the haystack — as this did — makes the
/// contract a lie for any caller passing a marker with an uppercase character:
/// it would silently never match. Every current call site happens to pass
/// lowercase constants, which is exactly why the bug would have gone unnoticed.
pub fn count_markers(haystack: &str, markers: &[&str]) -> usize {
    let lowered = haystack.to_lowercase();
    markers
        .iter()
        .filter(|m| lowered.contains(&m.to_lowercase()))
        .count()
}

/// Whether every marker in `markers` appears, case-insensitively.
pub fn all_markers(haystack: &str, markers: &[&str]) -> bool {
    let lowered = haystack.to_lowercase();
    markers.iter().all(|m| lowered.contains(&m.to_lowercase()))
}

/// Whether any marker in `markers` appears, case-insensitively.
pub fn any_marker(haystack: &str, markers: &[&str]) -> bool {
    let lowered = haystack.to_lowercase();
    markers.iter().any(|m| lowered.contains(&m.to_lowercase()))
}

/// Whether `marker` appears on a line that contains none of `excluding`.
///
/// The prompt a recipe just typed is echoed back on screen, so a bare
/// "does the output contain X" check passes on the echo alone. Excluding the
/// lines that caused the output is what makes the assertion mean anything.
///
/// **Case-sensitive**, unlike the marker helpers above. Deliberately so: callers
/// pass literal tokens they told the product to emit (`hello_from_bash_tool`)
/// and literal command text to exclude, and matching those loosely would let a
/// differently-cased near-miss count as real output. Do not reuse a marker
/// constant across this and [`any_marker`] without checking the case.
pub fn marker_on_unrelated_line(screen: &str, marker: &str, excluding: &[&str]) -> bool {
    screen
        .lines()
        .any(|line| line.contains(marker) && !excluding.iter().any(|e| line.contains(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_anchored_verbs_ignore_prose() {
        let d = FailureDetector::new();
        assert!(d.detects("Traceback (most recent call last):"));
        assert!(d.detects("  error: something broke"));
        // The whole point of anchoring: prose mentioning the word is not a failure.
        assert!(!d.detects("I will explain what an exception is in Python."));
        assert!(!d.detects("no errors were found"));
    }

    #[test]
    fn line_verbs_respect_word_boundaries() {
        let d = FailureDetector::new();
        assert!(d.detects("panic: at the disco"));
        assert!(d.detects("panic"));
        // The regexes this replaced spelled `panic\b`; a longer word is not a hit.
        assert!(!d.detects("panicking is unhelpful"));
        assert!(!d.detects("tracebacks explained"));
        // Punctuation-terminated verbs carry their own boundary.
        assert!(d.detects("error: boom"));
    }

    #[test]
    fn word_matching_sits_between_anchored_and_substring() {
        const W: [&str; 1] = ["panic"];
        let d = FailureDetector::markers_only().anywhere(&[]).words(&W);
        assert!(
            d.detects("the process hit a panic and died"),
            "not line-anchored"
        );
        assert!(
            !d.detects("panicking is unhelpful"),
            "but still word-bounded"
        );
    }

    #[test]
    fn substring_markers_match_anywhere() {
        const M: [&str; 1] = ["no results found"];
        let d = FailureDetector::markers_only().anywhere(&M);
        assert!(d.detects("Search returned: No Results Found."));
        assert!(
            !d.detects("a panic here"),
            "markers_only drops the line-anchored verb set"
        );
    }

    #[test]
    fn the_two_strategies_compose() {
        const M: [&str; 1] = ["unknown command"];
        let d = FailureDetector::new().anywhere(&M);
        assert!(d.detects("Unknown command: /nope"));
        assert!(d.detects("panic: at the disco"));
        assert!(!d.detects("all good"));
    }

    #[test]
    fn tool_result_errors_can_be_excluded() {
        const M: [&str; 1] = ["error:"];
        let screen = "SFT Submission Status\nmemory_write └ error: disabled";

        assert!(FailureDetector::markers_only().anywhere(&M).detects(screen));
        assert!(
            !FailureDetector::markers_only()
                .anywhere(&M)
                .ignoring_tool_result_errors()
                .detects(screen),
            "another tool failing is not this recipe's concern"
        );
    }

    #[test]
    fn anchoring_already_excludes_tool_result_errors() {
        // A `└ error:` line does not start with a verb, so the line-anchored
        // form never saw it in the first place. `ignoring_tool_result_errors`
        // is therefore only load-bearing for the substring markers — worth
        // knowing before adding it to a detector that does not need it.
        let screen = "SFT Submission Status\nmemory_write └ error: disabled";
        assert!(!FailureDetector::new().detects(screen));
    }

    #[test]
    fn a_custom_verb_set_replaces_the_default() {
        const V: [&str; 1] = ["permission denied"];
        let d = FailureDetector::new().line_verbs(&V);
        assert!(d.detects("Permission denied: /etc/shadow"));
        assert!(!d.detects("Traceback"), "default verbs were replaced");
    }

    #[test]
    fn the_core_set_is_the_default_for_markers_only() {
        let d = FailureDetector::markers_only();
        // Unanchored, unlike FailureDetector::new().
        assert!(d.detects("the tool reported error: boom"));
        assert!(d.detects("Traceback (most recent call last)"));
        assert!(!d.detects("all good"));
    }

    #[test]
    fn extra_markers_add_to_the_core_rather_than_replacing_it() {
        const E: [&str; 1] = ["no results found"];
        let d = FailureDetector::markers_only().extra(&E);
        assert!(d.detects("No results found"), "the extra marker");
        assert!(d.detects("Traceback"), "and still the core set");
    }

    #[test]
    fn markers_are_matched_case_insensitively_on_both_sides() {
        // The bug this pins: only the haystack was lowered, so an uppercase
        // marker silently never matched despite the documented contract.
        assert!(any_marker("no results found", &["No Results Found"]));
        assert!(any_marker("NO RESULTS FOUND", &["no results found"]));
        assert_eq!(count_markers("Alpha Beta", &["ALPHA", "beta"]), 2);
        assert!(all_markers("alpha beta", &["Alpha", "BETA"]));

        // And through the detector, which shares the contract.
        const M: [&str; 1] = ["No Results Found"];
        assert!(FailureDetector::markers_only()
            .anywhere(&M)
            .detects("search: no results found"));
    }

    #[test]
    fn unrelated_line_matching_is_case_sensitive_on_purpose() {
        // Callers pass literal tokens they told the product to emit, so a
        // differently-cased near-miss must not count as real output.
        let screen = "> Run echo hello_marker\nHELLO_MARKER\n";
        assert!(!marker_on_unrelated_line(
            screen,
            "hello_marker",
            &["Run echo"]
        ));
        let exact = "> Run echo hello_marker\nhello_marker\n";
        assert!(marker_on_unrelated_line(
            exact,
            "hello_marker",
            &["Run echo"]
        ));
    }

    #[test]
    fn marker_counting() {
        assert_eq!(count_markers("Alpha Beta", &["alpha", "beta", "gamma"]), 2);
        assert!(all_markers("Alpha Beta", &["alpha", "beta"]));
        assert!(!all_markers("Alpha", &["alpha", "beta"]));
        assert!(any_marker("Alpha", &["alpha", "beta"]));
        assert!(!any_marker("Delta", &["alpha", "beta"]));
    }

    #[test]
    fn prompt_echo_is_excluded_from_output_checks() {
        // The marker appears only on the line that typed it — not real output.
        let echo_only = "> Run echo hello_marker\n";
        assert!(!marker_on_unrelated_line(
            echo_only,
            "hello_marker",
            &["Run echo"]
        ));
        // Now it also appears in the tool's output.
        let with_output = "> Run echo hello_marker\nhello_marker\n";
        assert!(marker_on_unrelated_line(
            with_output,
            "hello_marker",
            &["Run echo"]
        ));
    }
}
