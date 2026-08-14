//! Python 3 `re` dialect for `wait_for_regex`.
//!
//! `specs/002-builtin-steps/spec.md` FR-019 fixes the pattern dialect as
//! Python's `re`, "not PCRE and not the Rust `regex` crate", and tabulates
//! thirteen patterns executed against the oracle. The `regex` crate fails
//! three of them outright — it has no lookaround and no backreferences — so
//! the engine underneath is `fancy-regex`, which backtracks.
//!
//! That still leaves three rows where a backtracking engine with PCRE heritage
//! disagrees with Python *silently*, which is the worse failure: the pattern
//! compiles and quietly means something else.
//!
//! | Pattern | Python | `fancy-regex` unaided |
//! |---|---|---|
//! | `x\Z` on `"x\n"` | no match — `\Z` is absolute end-of-string | matches — PCRE's "before a final newline" |
//! | `\p{L}` | raises | matches any letter |
//! | `a{2,1}` | raises | reads it as `a{1,2}` |
//!
//! So patterns are translated before they reach the engine, and the two forms
//! Python refuses are refused here too. Nothing else is rewritten: every other
//! row of FR-019 is a row `fancy-regex` already gets right.

use fancy_regex::Regex;

/// Compile a pattern written in Python's `re` dialect.
///
/// The error is a single line by construction — constitution Principle VIII
/// forbids the multi-line ASCII-art parse errors the `regex` crate emits.
///
/// The wording is TermProof's own. Byte-parity with CPython's `re.error` text
/// is a separate question, open as 001-OQ-001 / 002-OQ-002 / 003-OQ-010.
pub fn compile(pattern: &str) -> Result<Regex, String> {
    let translated = translate(pattern)?;
    Regex::new(&translated).map_err(|e| one_line(&e.to_string()))
}

fn one_line(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Rewrite a Python pattern into the equivalent `fancy-regex` pattern, or
/// reject it the way Python's parser would.
///
/// Positions in the messages count characters from the start of the pattern,
/// as Python's do.
fn translate(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0;
    let mut in_class = false;

    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let Some(&next) = chars.get(i + 1) else {
                    return Err(format!("trailing backslash at position {i}"));
                };
                // Python's `re` has no `\p{...}`; it raises rather than
                // matching, so a recipe using one must not quietly start
                // meaning "any letter".
                if next == 'p' || next == 'P' {
                    return Err(format!("unsupported escape \\{next} at position {i}"));
                }
                // Python's `\Z` is the absolute end of the string. The engine's
                // `\Z` also matches before a trailing newline; `\z` does not.
                if next == 'Z' && !in_class {
                    out.push_str("\\z");
                } else {
                    out.push('\\');
                    out.push(next);
                }
                i += 2;
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                i += 1;
                if chars.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                // A `]` immediately after `[` or `[^` is a literal in Python.
                // The engine needs it escaped to read it the same way.
                if chars.get(i) == Some(&']') {
                    out.push_str("\\]");
                    i += 1;
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
                i += 1;
            }
            '{' if !in_class => match repetition_at(&chars, i) {
                Some(rep) => {
                    if let (Some(min), Some(max)) = (rep.min, rep.max) {
                        if min > max {
                            return Err(format!("repetition range is inverted at position {i}"));
                        }
                    }
                    // `{,n}` means `{0,n}` in Python and is a literal brace to
                    // the engine.
                    if rep.min.is_none() {
                        out.push_str("{0,");
                        out.push_str(&rep.max.unwrap_or_default().to_string());
                        out.push('}');
                    } else {
                        out.extend(chars[i..rep.end].iter());
                    }
                    i = rep.end;
                }
                // Not a quantifier, so a literal brace — which Python accepts
                // bare and the engine does not.
                None => {
                    out.push_str("\\{");
                    i += 1;
                }
            },
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// A `{m,n}` quantifier, as far as Python's parser is concerned.
struct Repetition {
    /// One past the closing brace.
    end: usize,
    min: Option<u64>,
    max: Option<u64>,
}

/// Parse a repetition starting at `start` (which must index a `{`).
///
/// Returns `None` when the braces are not a quantifier at all — `a{`, `a{}`,
/// `a{,}` — which Python reads as literal text.
fn repetition_at(chars: &[char], start: usize) -> Option<Repetition> {
    let mut i = start + 1;
    let min_digits = digits_at(chars, &mut i);
    let comma = chars.get(i) == Some(&',');
    if comma {
        i += 1;
    }
    let max_digits = if comma {
        digits_at(chars, &mut i)
    } else {
        None
    };
    if chars.get(i) != Some(&'}') {
        return None;
    }
    if min_digits.is_none() && max_digits.is_none() {
        return None;
    }
    Some(Repetition {
        end: i + 1,
        min: min_digits,
        max: max_digits,
    })
}

/// Consume a run of ASCII digits, advancing `i`. A run too long for `u64` is
/// not a quantifier any engine will accept, so it reads as no digits at all.
fn digits_at(chars: &[char], i: &mut usize) -> Option<u64> {
    let start = *i;
    while chars.get(*i).is_some_and(|c| c.is_ascii_digit()) {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    chars[start..*i].iter().collect::<String>().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_match(pattern: &str, haystack: &str) -> bool {
        compile(pattern)
            .expect("pattern should compile")
            .is_match(haystack)
            .expect("match should not error")
    }

    #[test]
    fn translates_the_end_of_string_anchor() {
        assert_eq!(translate("x\\Z").unwrap(), "x\\z");
        assert!(!is_match("x\\Z", "x\n"));
        assert!(is_match("x\\Z", "x"));
    }

    #[test]
    fn leaves_other_escapes_alone() {
        assert_eq!(translate("\\A\\w+\\d\\\\").unwrap(), "\\A\\w+\\d\\\\");
    }

    #[test]
    fn rejects_unicode_property_escapes() {
        assert_eq!(
            compile("\\p{L}").unwrap_err(),
            "unsupported escape \\p at position 0"
        );
        assert_eq!(
            compile("ab\\P{L}").unwrap_err(),
            "unsupported escape \\P at position 2"
        );
    }

    /// An escaped backslash must not make the next character look escaped.
    #[test]
    fn a_doubled_backslash_does_not_swallow_the_next_escape() {
        assert!(compile("\\\\p").is_ok());
        assert!(compile("\\\\\\p{L}").is_err());
    }

    #[test]
    fn rejects_an_inverted_repetition_range() {
        assert_eq!(
            compile("a{2,1}").unwrap_err(),
            "repetition range is inverted at position 1"
        );
        assert!(compile("a{1,2}").is_ok());
        assert!(compile("a{2,}").is_ok());
        assert!(compile("a{2}").is_ok());
    }

    #[test]
    fn an_open_ended_lower_bound_means_zero() {
        assert_eq!(translate("a{,3}").unwrap(), "a{0,3}");
        assert!(is_match("^a{,3}$", "aa"));
    }

    #[test]
    fn a_brace_that_is_not_a_quantifier_is_a_literal() {
        assert!(is_match("a{", "a{"));
        assert!(is_match("a{}", "a{}"));
        assert!(is_match("a{,}", "a{,}"));
    }

    #[test]
    fn braces_inside_a_character_class_are_literal() {
        assert!(is_match("[{2,1}]", "{"));
        assert!(is_match("[{2,1}]", ","));
    }

    #[test]
    fn a_leading_close_bracket_is_a_class_member() {
        assert!(is_match("[]]", "]"));
        assert!(is_match("[^]]", "a"));
        assert!(!is_match("[^]]", "]"));
    }

    #[test]
    fn rejects_a_property_escape_inside_a_class() {
        assert!(compile("[\\p{L}]").is_err());
    }

    #[test]
    fn reports_a_trailing_backslash_rather_than_dropping_it() {
        assert_eq!(
            compile("ab\\").unwrap_err(),
            "trailing backslash at position 2"
        );
    }

    #[test]
    fn engine_errors_stay_on_one_line() {
        let err = compile("[bad").unwrap_err();
        assert!(!err.contains('\n'), "{err:?}");
    }

    #[test]
    fn counts_positions_in_characters_not_bytes() {
        assert_eq!(
            compile("é\\p{L}").unwrap_err(),
            "unsupported escape \\p at position 1"
        );
    }
}
