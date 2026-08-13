//! One Python `repr` formatter, shared by every diagnostic.
//!
//! Constitution Principle VIII: no Rust `Debug` rendering may appear in a
//! `detail`, and there is to be exactly one formatter rather than a `{:?}` at
//! each call site. Spec 002 FR-028 restates it; FR-020 is the reason it
//! matters, being "the most `repr`-dependent string in the product".
//!
//! Rust's `{:?}` and Python's `repr` agree often enough to look interchangeable
//! and disagree in four ways that all reach the report:
//!
//! | Value | `{:?}` | `repr` |
//! |---|---|---|
//! | `hello` | `"hello"` | `'hello'` |
//! | `it's` | `"it's"` | `"it's"` — the quote flips only here |
//! | `1.0` | `1.0` | `1.0` |
//! | `10.0` (as `f64` via `{}`) | `10` | `10.0` |
//! | `1e19` (via `{}`) | `10000000000000000000` | `1e+19` |
//!
//! The float rules are CPython's `repr` rules, not a borrowed string: shortest
//! round-trip digits, exponent form when the decimal point sits outside
//! `(-4, 16]`, and a `.0` on anything integral.

use serde_json::Value as JsonValue;

/// Python's `repr` of a string.
///
/// Single quotes, unless the value contains a `'` and no `"` — then the
/// delimiter flips to `"` and the apostrophes are left bare. That flip is what
/// makes `repr` unlike every Rust escape.
///
/// Escaping covers the cases `str.isprintable()` rules out in ASCII: the C0
/// controls, `DEL`, and the C1 range. Non-ASCII printable text is emitted as
/// itself, as Python does. Deciding printability for the whole of Unicode needs
/// the character database, which the port does not carry; the gap is confined
/// to formatting characters and unassigned code points, which Python would
/// escape and this renders literally.
pub fn repr_str(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if (0x80..=0x9f).contains(&(c as u32)) => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `repr` of a float.
///
/// `{}` gives Rust's own shortest round-trip form, which drops the `.0` from
/// integral values and spells `1e19` out in full. `{:e}` gives the same digits
/// in a form that can be re-laid-out to CPython's rules, which is what this
/// does.
pub fn repr_f64(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }

    // `{:e}` is shortest-round-trip scientific: `5e-2`, `1e0`, `1.25e0`.
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("`{:e}` always emits an exponent");
    let exponent: i32 = exponent.parse().expect("`{:e}` emits a decimal exponent");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();

    // Where the decimal point falls, counting from the left of `digits`.
    let point = exponent + 1;

    // CPython's threshold for switching to exponent form, from
    // `format_float_short` in repr mode.
    if point <= -4 || point > 16 {
        let mut out = String::from(sign);
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let exponent = point - 1;
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.abs()));
        return out;
    }

    let mut out = String::from(sign);
    if point <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-point) as usize));
        out.push_str(&digits);
    } else if (point as usize) >= digits.len() {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', point as usize - digits.len()));
        out.push_str(".0");
    } else {
        out.push_str(&digits[..point as usize]);
        out.push('.');
        out.push_str(&digits[point as usize..]);
    }
    out
}

/// Python's `repr` of a tuple. One element keeps a trailing comma — `('a',)` —
/// which is the detail FR-020 calls out and the easiest one to lose.
pub fn repr_tuple(items: &[String]) -> String {
    match items {
        [] => "()".to_string(),
        [only] => format!("({only},)"),
        many => format!("({})", many.join(", ")),
    }
}

/// Python's `repr` of a value that arrived as JSON.
///
/// JSON's types and Python's line up one for one after `json.loads`, so this is
/// what the oracle would print for the same recipe input.
pub fn repr_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "None".to_string(),
        JsonValue::Bool(true) => "True".to_string(),
        JsonValue::Bool(false) => "False".to_string(),
        JsonValue::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => i.to_string(),
            (_, Some(u)) => u.to_string(),
            // Only a float is left, and a JSON number is always finite.
            _ => repr_f64(n.as_f64().unwrap_or(f64::NAN)),
        },
        JsonValue::String(s) => repr_str(s),
        JsonValue::Array(items) => format!(
            "[{}]",
            items.iter().map(repr_json).collect::<Vec<_>>().join(", ")
        ),
        JsonValue::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }
            let pairs = map
                .iter()
                .map(|(k, v)| format!("{}: {}", repr_str(k), repr_json(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{pairs}}}")
        }
    }
}

/// The Python type name of a value that arrived as JSON, as `type(x).__name__`
/// would give it.
///
/// JSON has one number type and Python has two, so the split is on whether the
/// literal had a fraction or exponent — which is the same split `json.loads`
/// makes.
pub fn type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "NoneType",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(n) => {
            if n.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        JsonValue::String(_) => "str",
        JsonValue::Array(_) => "list",
        JsonValue::Object(_) => "dict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strings_use_single_quotes() {
        assert_eq!(repr_str("hello"), "'hello'");
        assert_eq!(repr_str(""), "''");
    }

    /// The one case where the delimiter changes, and the reason a plain
    /// `{:?}` cannot stand in.
    #[test]
    fn an_apostrophe_flips_the_delimiter() {
        assert_eq!(repr_str("it's"), "\"it's\"");
    }

    /// With both quote characters present Python keeps single quotes and
    /// escapes the apostrophes.
    #[test]
    fn both_quote_characters_keep_single_quotes() {
        assert_eq!(repr_str("it's \"quoted\""), "'it\\'s \"quoted\"'");
    }

    #[test]
    fn a_double_quote_alone_needs_no_escaping() {
        assert_eq!(repr_str("say \"hi\""), "'say \"hi\"'");
    }

    #[test]
    fn escapes_the_characters_python_escapes() {
        assert_eq!(repr_str("a\nb\tc\rd\\e"), "'a\\nb\\tc\\rd\\\\e'");
        assert_eq!(repr_str("\u{0}\u{1b}\u{7f}"), "'\\x00\\x1b\\x7f'");
    }

    #[test]
    fn leaves_printable_non_ascii_alone() {
        assert_eq!(repr_str("héllo →"), "'héllo →'");
    }

    #[test]
    fn integral_floats_keep_their_point() {
        assert_eq!(repr_f64(1.0), "1.0");
        assert_eq!(repr_f64(10.0), "10.0");
        assert_eq!(repr_f64(0.0), "0.0");
        assert_eq!(repr_f64(-0.0), "-0.0");
        assert_eq!(repr_f64(100.0), "100.0");
    }

    #[test]
    fn fractions_round_trip_shortest() {
        assert_eq!(repr_f64(0.5), "0.5");
        assert_eq!(repr_f64(0.05), "0.05");
        assert_eq!(repr_f64(1.25), "1.25");
        assert_eq!(repr_f64(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(repr_f64(-1.5), "-1.5");
    }

    /// CPython switches to exponent form when the decimal point falls outside
    /// `(-4, 16]`, and pads the exponent to two digits.
    #[test]
    fn large_and_small_magnitudes_use_exponent_form() {
        assert_eq!(repr_f64(1e19), "1e+19");
        assert_eq!(repr_f64(1e300), "1e+300");
        assert_eq!(repr_f64(1e16), "1e+16");
        assert_eq!(repr_f64(1e15), "1000000000000000.0");
        assert_eq!(repr_f64(0.0001), "0.0001");
        assert_eq!(repr_f64(0.00001), "1e-05");
        assert_eq!(repr_f64(1.5e-7), "1.5e-07");
        assert_eq!(repr_f64(-1e300), "-1e+300");
    }

    #[test]
    fn the_non_finite_values_have_names_not_symbols() {
        assert_eq!(repr_f64(f64::NAN), "nan");
        assert_eq!(repr_f64(f64::INFINITY), "inf");
        assert_eq!(repr_f64(f64::NEG_INFINITY), "-inf");
    }

    /// FR-020 calls this out by name.
    #[test]
    fn a_one_element_tuple_keeps_its_comma() {
        assert_eq!(repr_tuple(&["'a'".into()]), "('a',)");
        assert_eq!(repr_tuple(&["'a'".into(), "'b'".into()]), "('a', 'b')");
        assert_eq!(repr_tuple(&[]), "()");
    }

    #[test]
    fn json_values_render_as_python_literals() {
        assert_eq!(repr_json(&json!(null)), "None");
        assert_eq!(repr_json(&json!(true)), "True");
        assert_eq!(repr_json(&json!(false)), "False");
        assert_eq!(repr_json(&json!(5)), "5");
        assert_eq!(repr_json(&json!(-5)), "-5");
        assert_eq!(repr_json(&json!(0.05)), "0.05");
        assert_eq!(repr_json(&json!("abc")), "'abc'");
        assert_eq!(repr_json(&json!([])), "[]");
        assert_eq!(repr_json(&json!([1, "a", null])), "[1, 'a', None]");
        assert_eq!(repr_json(&json!({})), "{}");
        assert_eq!(repr_json(&json!({"a": 1})), "{'a': 1}");
    }

    /// A JSON number carries no float/int distinction of its own; the literal's
    /// shape is what `json.loads` uses too.
    #[test]
    fn number_types_split_the_way_json_loads_splits_them() {
        assert_eq!(type_name(&json!(5)), "int");
        assert_eq!(type_name(&json!(5.0)), "float");
        assert_eq!(type_name(&json!(0.05)), "float");
        assert_eq!(type_name(&json!(null)), "NoneType");
        assert_eq!(type_name(&json!(true)), "bool");
        assert_eq!(type_name(&json!("s")), "str");
        assert_eq!(type_name(&json!([])), "list");
        assert_eq!(type_name(&json!({})), "dict");
    }
}
