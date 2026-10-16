//! English texts of the Go jsonschema library (santhosh-tekuri/jsonschema/v6,
//! kind/kind.go) and the number formatting of golang.org/x/text/message with
//! language.English, which that library prints its messages with.

use serde_json::Value;

use crate::config;

/// go_quote is Go's %q of a string (strconv.Quote).
pub(super) use config::go_quote;

/// quote is the library's quote: Go's %q with the outer double quotes
/// turned into single quotes, a double quote left bare and a single quote
/// escaped. As in Go, \" is replaced in the whole quoted string, so a value
/// ending in a backslash loses one: `ab\` prints as 'ab\'.
pub(super) fn quote(s: &str) -> String {
    let q = go_quote(s).replace("\\\"", "\"").replace('\'', "\\'");
    format!("'{}'", &q[1..q.len() - 1])
}

/// quote_rune is Go's %q of a rune (strconv.QuoteRune).
pub(super) fn quote_rune(c: char) -> String {
    let q = go_quote(&c.to_string());
    let inner = q[1..q.len() - 1].replace("\\\"", "\"").replace('\'', "\\'");
    format!("'{inner}'")
}

/// join_quoted quotes every string and joins them with sep.
pub(super) fn join_quoted(arr: &[String], sep: &str) -> String {
    arr.iter().map(|s| quote(s)).collect::<Vec<_>>().join(sep)
}

/// display prints a primitive the way the library does: strings quoted,
/// arrays and objects as "value", the rest with Go's %v.
pub(super) fn display(v: &Value) -> String {
    match v {
        Value::String(s) => quote(s),
        Value::Array(_) | Value::Object(_) => "value".to_string(),
        Value::Null => "<nil>".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => go_v_float(n.as_f64().unwrap_or(0.0)),
        },
    }
}

/// type_of is the JSON type name the library reports as "got": every number
/// is "number".
pub(super) fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// TYPE_ORDER is the order the library lists wanted types in.
pub(super) const TYPE_ORDER: [&str; 7] = [
    "null", "boolean", "number", "integer", "string", "array", "object",
];

/// digits returns the shortest decimal digits of |f| and the exponent of the
/// first digit (1234.5 -> "12345", 3).
fn digits(f: f64) -> (String, i32) {
    let e = format!("{:e}", f.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    (mant.replace('.', ""), exp.parse().unwrap_or(0))
}

/// go_v_float is Go's %v of a float64 (strconv 'g' with the shortest
/// precision): the exponent form below 1e-4 and from 1e6 on, like
/// "1.234567e+06" and "1e-05".
pub(super) fn go_v_float(f: f64) -> String {
    if f == 0.0 || !f.is_finite() {
        return format!("{f}");
    }
    let (d, exp) = digits(f);
    if !(-4..6).contains(&exp) {
        let sign = if f < 0.0 { "-" } else { "" };
        let mant = if d.len() > 1 {
            format!("{}.{}", &d[..1], &d[1..])
        } else {
            d.clone()
        };
        let es = if exp < 0 { '-' } else { '+' };
        return format!("{sign}{mant}e{es}{:02}", exp.abs());
    }
    format!("{f}")
}

/// group writes the integer digits with a comma every three digits.
fn group(int: &str) -> String {
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// printer_int is %d of the English message printer: 1000 -> "1,000".
pub(super) fn printer_int(i: i64) -> String {
    let sign = if i < 0 { "-" } else { "" };
    format!("{sign}{}", group(&i.unsigned_abs().to_string()))
}

const SUPERSCRIPT: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// printer_float is %v of a float64 with the English message printer: the
/// shortest digits, grouped as 1,500.5, or "1.5 × 10⁰⁶" (narrow no-break
/// spaces, at least two exponent digits) when the exponent is below -4 or
/// at least 6.
pub(super) fn printer_float(f: f64) -> String {
    if f == 0.0 {
        return "0".to_string();
    }
    let sign = if f < 0.0 { "-" } else { "" };
    let (d, exp) = digits(f);
    if !(-4..6).contains(&exp) {
        let mant = if d.len() > 1 {
            format!("{}.{}", &d[..1], &d[1..])
        } else {
            d.clone()
        };
        let mut e = String::new();
        if exp < 0 {
            e.push('⁻');
        }
        for c in format!("{:02}", exp.abs()).chars() {
            e.push(SUPERSCRIPT[c as usize - '0' as usize]);
        }
        return format!("{sign}{mant}\u{202f}×\u{202f}10{e}");
    }
    let s = format!("{}", f.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let frac = if frac.is_empty() {
        String::new()
    } else {
        format!(".{frac}")
    };
    format!("{sign}{}{frac}", group(int))
}
