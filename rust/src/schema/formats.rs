//! The "format" checks of the Go jsonschema library (format.go) with their
//! error texts, and the parts of the Go standard library they rely on:
//! time.Parse with the layout "2006-01-02", strconv.Atoi, netip.ParseAddr and
//! url.Parse. A failing check returns the text the library prints after
//! "is not valid <format>: ".

use super::texts::go_quote;
use super::texts::{printer_int, quote_rune};

/// NAMES are the formats the library asserts; any other format is ignored.
pub(super) const NAMES: [&str; 19] = [
    "json-pointer",
    "relative-json-pointer",
    "uuid",
    "duration",
    "period",
    "ipv4",
    "ipv6",
    "hostname",
    "email",
    "date",
    "time",
    "date-time",
    "uri",
    "iri",
    "uri-reference",
    "iri-reference",
    "uri-template",
    "semver",
    "regex",
];

type Check = Result<(), String>;

/// validate checks s against the named format; None for a format the
/// library does not know.
pub(super) fn validate(name: &str, s: &str) -> Option<Check> {
    let b = s.as_bytes();
    Some(match name {
        "json-pointer" => validate_json_pointer(b),
        "relative-json-pointer" => validate_relative_json_pointer(b),
        "uuid" => validate_uuid(s),
        "duration" => validate_duration(b),
        "period" => validate_period(b),
        "ipv4" => validate_ipv4(b),
        "ipv6" => validate_ipv6(b),
        "hostname" => validate_hostname(s),
        "email" => validate_email(s),
        "date" => validate_date(b),
        "time" => validate_time(b),
        "date-time" => validate_date_time(b),
        "uri" | "iri" => validate_uri(s),
        "uri-reference" | "iri-reference" => validate_uri_reference(s),
        "uri-template" => validate_uri_template(s),
        "semver" => validate_semver(s),
        "regex" => crate::config::compile_go_regexp(s).map(|_| ()),
        _ => return None,
    })
}

/// lossy is a byte slice as text for the checks that look at characters.
fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// go_quote_bytes is strconv.Quote of bytes that may be invalid UTF-8: an
/// invalid byte is written as \xNN.
fn go_quote_bytes(b: &[u8]) -> String {
    let mut out = String::from("\"");
    for chunk in b.utf8_chunks() {
        let q = go_quote(chunk.valid());
        out.push_str(&q[1..q.len() - 1]);
        for x in chunk.invalid() {
            out.push_str(&format!("\\x{x:02x}"));
        }
    }
    out.push('"');
    out
}

// see https://www.rfc-editor.org/rfc/rfc6901#section-3
fn validate_json_pointer(s: &[u8]) -> Check {
    if s.is_empty() {
        return Ok(());
    }
    if s[0] != b'/' {
        return Err("not starting with /".into());
    }
    for tok in lossy(s).split('/').skip(1) {
        let mut escape = false;
        for ch in tok.chars() {
            if escape {
                escape = false;
                if ch != '0' && ch != '1' {
                    return Err("~ must be followed by 0 or 1".into());
                }
                continue;
            }
            if ch == '~' {
                escape = true;
                continue;
            }
            match ch {
                '\x00'..='\x2E' | '\x30'..='\x7D' | '\x7F'..='\u{10FFFF}' => {}
                _ => return Err(format!("invalid character {}", quote_rune(ch))),
            }
        }
        if escape {
            return Err("~ must be followed by 0 or 1".into());
        }
    }
    Ok(())
}

/// leading_digits counts the ASCII digits at the start of s.
fn leading_digits(s: &[u8]) -> usize {
    s.iter().take_while(|c| c.is_ascii_digit()).count()
}

// see https://tools.ietf.org/html/draft-handrews-relative-json-pointer-01#section-3
fn validate_relative_json_pointer(s: &[u8]) -> Check {
    // start with non-negative-integer
    let num_digits = leading_digits(s);
    if num_digits == 0 {
        return Err("must start with non-negative integer".into());
    }
    if num_digits > 1 && s[0] == b'0' {
        return Err("starts with zero".into());
    }
    let s = &s[num_digits..];

    // followed by either json-pointer or '#'
    if s == b"#" {
        return Ok(());
    }
    validate_json_pointer(s)
}

// see https://datatracker.ietf.org/doc/html/rfc4122#page-4
fn validate_uuid(s: &str) -> Check {
    let hex_groups = [8usize, 4, 4, 4, 12];
    let groups: Vec<&str> = s.split('-').collect();
    if groups.len() != hex_groups.len() {
        return Err(format!(
            "must have {} elements",
            printer_int(hex_groups.len() as i64)
        ));
    }
    for (i, group) in groups.iter().enumerate() {
        if group.len() != hex_groups[i] {
            return Err(format!(
                "element {} must be {} characters long",
                printer_int(i as i64 + 1),
                printer_int(hex_groups[i] as i64)
            ));
        }
        for ch in group.chars() {
            if !ch.is_ascii_hexdigit() {
                return Err(format!("non-hex character {}", quote_rune(ch)));
            }
        }
    }
    Ok(())
}

// see https://datatracker.ietf.org/doc/html/rfc3339#appendix-A
fn validate_duration(s: &[u8]) -> Check {
    // must start with 'P'
    let Some(s) = s.strip_prefix(b"P") else {
        return Err("must start with P".into());
    };
    if s.is_empty() {
        return Err("nothing after P".into());
    }

    // dur-week
    if let Some(s) = s.strip_suffix(b"W") {
        if s.is_empty() {
            return Err("no number in week".into());
        }
        if !s.iter().all(u8::is_ascii_digit) {
            return Err("invalid week".into());
        }
        return Ok(());
    }

    let all_units: [&[u8]; 2] = [b"YMD", b"HMS"];
    for (i, mut s) in s.split(|&c| c == b'T').enumerate() {
        if i != 0 && s.is_empty() {
            return Err("no time elements".into());
        }
        if i >= all_units.len() {
            return Err("more than one T".into());
        }
        let mut units = all_units[i];
        while !s.is_empty() {
            let digit_count = leading_digits(s);
            if digit_count == 0 {
                return Err("missing number".into());
            }
            s = &s[digit_count..];
            if s.is_empty() {
                return Err("missing unit".into());
            }
            let unit = s[0];
            let Some(j) = units.iter().position(|&u| u == unit) else {
                if all_units[i].contains(&unit) {
                    return Err(format!(
                        "unit {} out of order",
                        quote_rune(char::from(unit))
                    ));
                }
                return Err(format!("invalid unit {}", quote_rune(char::from(unit))));
            };
            units = &units[j + 1..];
            s = &s[1..];
        }
    }
    Ok(())
}

fn validate_ipv4(s: &[u8]) -> Check {
    let groups: Vec<&[u8]> = s.split(|&c| c == b'.').collect();
    if groups.len() != 4 {
        return Err("expected four decimals".into());
    }
    for group in groups {
        if group.len() > 1 && group[0] == b'0' {
            return Err("leading zeros".into());
        }
        let n = atoi(group)?;
        if !(0..=255).contains(&n) {
            return Err("decimal must be between 0 and 255".into());
        }
    }
    Ok(())
}

/// atoi is strconv.Atoi with its error text.
fn atoi(s: &[u8]) -> Result<i64, String> {
    let err = |what: &str| format!("strconv.Atoi: parsing {}: {what}", go_quote_bytes(s));
    let (neg, digits) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() {
        return Err(err("invalid syntax"));
    }
    let mut n: u64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return Err(err("invalid syntax"));
        }
        // ParseUint stops at the first overflow, before looking further
        n = match n
            .checked_mul(10)
            .and_then(|x| x.checked_add(u64::from(c - b'0')))
        {
            Some(x) => x,
            None => return Err(err("value out of range")),
        };
    }
    let limit = if neg { 1u64 << 63 } else { (1u64 << 63) - 1 };
    if n > limit {
        return Err(err("value out of range"));
    }
    Ok(if neg {
        (n as i64).wrapping_neg()
    } else {
        n as i64
    })
}

fn validate_ipv6(s: &[u8]) -> Check {
    if !s.contains(&b':') {
        return Err("missing colon".into());
    }
    let addr = parse_addr(s)?;
    if !addr.zone.is_empty() {
        return Err("zone id is not a part of ipv6 address".into());
    }
    Ok(())
}

// see https://en.wikipedia.org/wiki/Hostname#Restrictions_on_valid_host_names
fn validate_hostname(s: &str) -> Check {
    // entire hostname (including the delimiting dots but not a trailing dot) has a maximum of 253 ASCII characters
    let s = s.strip_suffix('.').unwrap_or(s);
    if s.len() > 253 {
        return Err("more than 253 characters long".into());
    }

    // Hostnames are composed of series of labels concatenated with dots, as are all domain names
    for label in s.split('.') {
        // Each label must be from 1 to 63 characters long
        if label.is_empty() || label.len() > 63 {
            return Err("label must be 1 to 63 characters long".into());
        }

        // labels must not start or end with a hyphen
        if label.starts_with('-') {
            return Err("label starts with hyphen".into());
        }
        if label.ends_with('-') {
            return Err("label ends with hyphen".into());
        }

        // labels may contain only the ASCII letters 'a' through 'z' (in a case-insensitive manner),
        // the digits '0' through '9', and the hyphen ('-')
        for ch in label.chars() {
            if !(ch.is_ascii_alphanumeric() || ch == '-') {
                return Err(format!("invalid character {}", quote_rune(ch)));
            }
        }
    }
    Ok(())
}

// see https://en.wikipedia.org/wiki/Email_address
fn validate_email(s: &str) -> Check {
    // entire email address to be no more than 254 characters long
    if s.len() > 254 {
        return Err("more than 255 characters long".into());
    }

    // email address is generally recognized as having two parts joined with an at-sign
    let Some(at) = s.rfind('@') else {
        return Err("missing @".into());
    };
    let (local, domain) = (&s[..at], &s[at + 1..]);

    // local part may be up to 64 characters long
    if local.len() > 64 {
        return Err("local part more than 64 characters long".into());
    }

    if local.len() > 1 && local.starts_with('"') && local.ends_with('"') {
        // quoted
        let local = &local[1..local.len() - 1];
        if local.contains('\\') || local.contains('"') {
            return Err("backslash and quote are not allowed within quoted local part".into());
        }
    } else {
        // unquoted
        if local.starts_with('.') {
            return Err("starts with dot".into());
        }
        if local.ends_with('.') {
            return Err("ends with dot".into());
        }

        // consecutive dots not allowed
        if local.contains("..") {
            return Err("consecutive dots".into());
        }

        // check allowed chars
        for ch in local.chars() {
            if !(ch.is_ascii_alphanumeric() || ".!#$%&'*+-/=?^_`{|}~".contains(ch)) {
                return Err(format!("invalid character {}", quote_rune(ch)));
            }
        }
    }

    // domain if enclosed in brackets, must match an IP address
    if domain.starts_with('[') && domain.ends_with(']') && domain.len() >= 2 {
        let domain = &domain[1..domain.len() - 1];
        if let Some(rem) = domain.strip_prefix("IPv6:") {
            if let Err(e) = validate_ipv6(rem.as_bytes()) {
                return Err(format!("invalid ipv6 address: {e}"));
            }
            return Ok(());
        }
        if let Err(e) = validate_ipv4(domain.as_bytes()) {
            return Err(format!("invalid ipv4 address: {e}"));
        }
        return Ok(());
    }

    // domain must match the requirements for a hostname
    if let Err(e) = validate_hostname(domain) {
        return Err(format!("invalid domain: {e}"));
    }
    Ok(())
}

// see https://datatracker.ietf.org/doc/html/rfc3339#section-5.6
fn validate_date(s: &[u8]) -> Check {
    parse_date(s)
}

// see https://datatracker.ietf.org/doc/html/rfc3339#section-5.6
// NOTE: golang time package does not support leap seconds.
fn validate_time(s: &[u8]) -> Check {
    let mut str = s;
    // min: hh:mm:ssZ
    if str.len() < 9 {
        return Err("less than 9 characters long".into());
    }
    if str[2] != b':' || str[5] != b':' {
        return Err("missing colon in correct place".into());
    }

    // parse hh:mm:ss
    let mut hms = Vec::new();
    for tok in str[..8].splitn(3, |&c| c == b':') {
        let Ok(i) = atoi(tok) else {
            return Err("invalid hour/min/sec".into());
        };
        if i < 0 {
            return Err("non-positive hour/min/sec".into());
        }
        hms.push(i);
    }
    if hms.len() != 3 {
        return Err("missing hour/min/sec".into());
    }
    let (mut h, mut m, s) = (hms[0], hms[1], hms[2]);
    if h > 23 || m > 59 || s > 60 {
        return Err("hour/min/sec out of range".into());
    }
    str = &str[8..];

    // parse sec-frac if present
    if let Some(rem) = str.strip_prefix(b".") {
        let num_digits = leading_digits(rem);
        if num_digits == 0 {
            return Err("no digits in second fraction".into());
        }
        str = &rem[num_digits..];
    }

    if str != b"z" && str != b"Z" {
        // parse time-numoffset
        if str.len() != 6 {
            return Err("offset must be 6 characters long".into());
        }
        let sign = match str[0] {
            b'+' => -1,
            b'-' => 1,
            _ => return Err("offset must begin with plus/minus".into()),
        };
        str = &str[1..];
        if str[2] != b':' {
            return Err("missing colon in offset in correct place".into());
        }

        let mut zhm = Vec::new();
        for tok in str.splitn(2, |&c| c == b':') {
            let Ok(i) = atoi(tok) else {
                return Err("invalid hour/min in offset".into());
            };
            if i < 0 {
                return Err("non-positive hour/min in offset".into());
            }
            zhm.push(i);
        }
        let (zh, zm) = (zhm[0], zhm[1]);
        if zh > 23 || zm > 59 {
            return Err("hour/min in offset out of range".into());
        }

        // apply timezone
        let mut hm = (h * 60 + m) + sign * (zh * 60 + zm);
        if hm < 0 {
            hm += 24 * 60;
        }
        (h, m) = (hm / 60, hm % 60);
    }

    // check leap second
    if s >= 60 && (h != 23 || m != 59) {
        return Err("invalid leap second".into());
    }
    Ok(())
}

// see https://datatracker.ietf.org/doc/html/rfc3339#section-5.6
fn validate_date_time(s: &[u8]) -> Check {
    // min: yyyy-mm-ddThh:mm:ssZ
    if s.len() < 20 {
        return Err("less than 20 characters long".into());
    }
    if s[10] != b't' && s[10] != b'T' {
        return Err("11th character must be t or T".into());
    }
    if let Err(e) = validate_date(&s[..10]) {
        return Err(format!("invalid date element: {e}"));
    }
    if let Err(e) = validate_time(&s[11..]) {
        return Err(format!("invalid time element: {e}"));
    }
    Ok(())
}

fn validate_uri(s: &str) -> Check {
    let u = parse_url(s)?;
    if u.scheme.is_empty() {
        return Err("relative url".into());
    }
    Ok(())
}

fn validate_uri_reference(s: &str) -> Check {
    if s.contains('\\') {
        return Err(r"contains \".into());
    }
    parse_url(s).map(|_| ())
}

fn validate_uri_template(s: &str) -> Check {
    let u = parse_url(s)?;
    for tok in u.raw_path.split(|&c| c == b'/') {
        let tok = match unescape(tok, Enc::PathSegment) {
            Ok(t) => t,
            Err(e) => return Err(format!("percent decode failed: {e}")),
        };
        let mut want = true;
        for ch in lossy(&tok).chars() {
            let got = match ch {
                '{' => true,
                '}' => false,
                _ => continue,
            };
            if got != want {
                return Err("nested curly braces".into());
            }
            want = !want;
        }
        if !want {
            return Err("no matching closing brace".into());
        }
    }
    Ok(())
}

fn validate_period(s: &[u8]) -> Check {
    let Some(slash) = s.iter().position(|&c| c == b'/') else {
        return Err("missing slash".into());
    };
    let (start, end) = (&s[..slash], &s[slash + 1..]);
    if start.starts_with(b"P") {
        if let Err(e) = validate_duration(start) {
            return Err(format!("invalid start duration: {e}"));
        }
        if let Err(e) = validate_date_time(end) {
            return Err(format!("invalid end date-time: {e}"));
        }
    } else {
        if let Err(e) = validate_date_time(start) {
            return Err(format!("invalid start date-time: {e}"));
        }
        if end.starts_with(b"P") {
            if let Err(e) = validate_duration(end) {
                return Err(format!("invalid end duration: {e}"));
            }
        } else if let Err(e) = validate_date_time(end) {
            return Err(format!("invalid end date-time: {e}"));
        }
    }
    Ok(())
}

// see https://semver.org/#backusnaur-form-grammar-for-valid-semver-versions
fn validate_semver(s: &str) -> Check {
    let mut s = s;
    // build --
    if let Some(i) = s.find('+') {
        let build = &s[i + 1..];
        if build.is_empty() {
            return Err("build is empty".into());
        }
        for build_id in build.split('.') {
            if build_id.is_empty() {
                return Err("build identifier is empty".into());
            }
            for ch in build_id.chars() {
                if !(ch.is_ascii_alphanumeric() || ch == '-') {
                    return Err(format!(
                        "invalid character {} in build identifier",
                        quote_rune(ch)
                    ));
                }
            }
        }
        s = &s[..i];
    }

    // pre-release --
    if let Some(i) = s.find('-') {
        let pre_release = &s[i + 1..];
        for id in pre_release.split('.') {
            if id.is_empty() {
                return Err("pre-release identifier is empty".into());
            }
            let mut all_digits = true;
            for ch in id.chars() {
                if ch.is_ascii_digit() {
                } else if ch.is_ascii_alphabetic() || ch == '-' {
                    all_digits = false;
                } else {
                    return Err(format!(
                        "invalid character {} in pre-release identifier",
                        quote_rune(ch)
                    ));
                }
            }
            if all_digits && id.len() > 1 && id.starts_with('0') {
                return Err("pre-release numeric identifier starts with zero".into());
            }
        }
        s = &s[..i];
    }

    // versionCore --
    let versions: Vec<&str> = s.split('.').collect();
    if versions.len() != 3 {
        return Err("versionCore must have 3 numbers separated by dot".into());
    }
    let names = ["major", "minor", "patch"];
    for (i, version) in versions.iter().enumerate() {
        if version.is_empty() {
            return Err(format!("{} is empty", names[i]));
        }
        if version.len() > 1 && version.starts_with('0') {
            return Err(format!("{} starts with zero", names[i]));
        }
        if !version.bytes().all(|c| c.is_ascii_digit()) {
            return Err(format!("{} contains non-digit", names[i]));
        }
    }
    Ok(())
}

// ---- time.Parse("2006-01-02", s) ----

/// time_quote is the quote of package time: every byte that is not
/// printable ASCII is written as \xNN.
fn time_quote(s: &[u8]) -> String {
    let mut out = String::from("\"");
    for &c in s {
        if !(b' '..0x80).contains(&c) {
            out.push_str(&format!("\\x{c:02x}"));
        } else {
            if c == b'"' || c == b'\\' {
                out.push('\\');
            }
            out.push(char::from(c));
        }
    }
    out.push('"');
    out
}

/// parse_date is time.Parse with the layout "2006-01-02" and its errors.
fn parse_date(avalue: &[u8]) -> Check {
    const LAYOUT: &[u8] = b"2006-01-02";
    let cannot = |value_elem: &[u8], layout_elem: &[u8]| {
        Err(format!(
            "parsing time {} as {}: cannot parse {} as {}",
            time_quote(avalue),
            time_quote(LAYOUT),
            time_quote(value_elem),
            time_quote(layout_elem)
        ))
    };
    let digit = |s: &[u8], i: usize| s.get(i).is_some_and(u8::is_ascii_digit);
    // getnum(s, true): exactly two digits
    let getnum = |s: &[u8]| -> Option<i64> {
        if !digit(s, 0) || !digit(s, 1) {
            return None;
        }
        Some(i64::from(s[0] - b'0') * 10 + i64::from(s[1] - b'0'))
    };
    let mut value = avalue;

    // 2006: four digits
    if value.len() < 4 || !digit(value, 0) || !value[..4].iter().all(u8::is_ascii_digit) {
        return cannot(value, b"2006");
    }
    let year: i64 = String::from_utf8_lossy(&value[..4]).parse().unwrap_or(0);
    value = &value[4..];

    // -01
    let Some(v) = value.strip_prefix(b"-") else {
        return cannot(value, b"-");
    };
    value = v;
    let Some(month) = getnum(value) else {
        return cannot(value, b"01");
    };
    value = &value[2..];
    if !(1..=12).contains(&month) {
        return Err(format!(
            "parsing time {}: month out of range",
            time_quote(avalue)
        ));
    }

    // -02
    let Some(v) = value.strip_prefix(b"-") else {
        return cannot(value, b"-");
    };
    value = v;
    let Some(day) = getnum(value) else {
        return cannot(value, b"02");
    };
    value = &value[2..];

    if !value.is_empty() {
        return Err(format!(
            "parsing time {}: extra text: {}",
            time_quote(avalue),
            time_quote(value)
        ));
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day < 1 || day > days {
        return Err(format!(
            "parsing time {}: day out of range",
            time_quote(avalue)
        ));
    }
    Ok(())
}

// ---- netip.ParseAddr ----

/// Addr is what the checks need from a parsed netip.Addr.
struct Addr {
    is4: bool,
    zone: String,
}

fn addr_err(input: &[u8], msg: &str, at: &[u8]) -> String {
    if at.is_empty() {
        return format!("ParseAddr({}): {msg}", go_quote_bytes(input));
    }
    format!(
        "ParseAddr({}): {msg} (at {})",
        go_quote_bytes(input),
        go_quote_bytes(at)
    )
}

/// parse_addr is netip.ParseAddr with its errors.
fn parse_addr(s: &[u8]) -> Result<Addr, String> {
    for &c in s {
        match c {
            b'.' => {
                return parse_ipv4_fields(s, 0, s.len()).map(|_| Addr {
                    is4: true,
                    zone: String::new(),
                });
            }
            b':' => return parse_ipv6(s),
            b'%' => return Err(addr_err(s, "missing IPv6 address", b"")),
            _ => {}
        }
    }
    Err(addr_err(s, "unable to parse IP", b""))
}

fn parse_ipv4_fields(input: &[u8], off: usize, end: usize) -> Result<(), String> {
    let (mut val, mut pos, mut dig_len) = (0u32, 0, 0);
    let s = &input[off..end];
    for i in 0..s.len() {
        if s[i].is_ascii_digit() {
            if dig_len == 1 && val == 0 {
                return Err(addr_err(
                    input,
                    "IPv4 field has octet with leading zero",
                    b"",
                ));
            }
            val = val * 10 + u32::from(s[i] - b'0');
            dig_len += 1;
            if val > 255 {
                return Err(addr_err(input, "IPv4 field has value >255", b""));
            }
        } else if s[i] == b'.' {
            // .1.2.3
            // 1.2.3.
            // 1..2.3
            if i == 0 || i == s.len() - 1 || s[i - 1] == b'.' {
                return Err(addr_err(
                    input,
                    "IPv4 field must have at least one digit",
                    &s[i..],
                ));
            }
            // 1.2.3.4.5
            if pos == 3 {
                return Err(addr_err(input, "IPv4 address too long", b""));
            }
            pos += 1;
            val = 0;
            dig_len = 0;
        } else {
            return Err(addr_err(input, "unexpected character", &s[i..]));
        }
    }
    if pos < 3 {
        return Err(addr_err(input, "IPv4 address too short", b""));
    }
    Ok(())
}

fn parse_ipv6(input: &[u8]) -> Result<Addr, String> {
    let mut s = input;

    // Split off the zone right from the start.
    let mut zone: &[u8] = b"";
    if let Some(i) = s.iter().position(|&c| c == b'%') {
        (s, zone) = (&s[..i], &s[i + 1..]);
        if zone.is_empty() {
            // Not allowed to have an empty zone if explicitly specified.
            return Err(addr_err(input, "zone must be a non-empty string", b""));
        }
    }
    let ok = |zone: &[u8]| {
        Ok(Addr {
            is4: false,
            zone: String::from_utf8_lossy(zone).into_owned(),
        })
    };

    let mut ellipsis: i32 = -1; // position of ellipsis in ip

    // Might have leading ellipsis
    if s.len() >= 2 && s[0] == b':' && s[1] == b':' {
        ellipsis = 0;
        s = &s[2..];
        // Might be only ellipsis
        if s.is_empty() {
            return ok(zone);
        }
    }

    // Loop, parsing hex numbers followed by colon.
    let mut i = 0;
    while i < 16 {
        let mut off = 0;
        let mut acc: u32 = 0;
        while off < s.len() {
            let c = s[off];
            let Some(d) = char::from(c).to_digit(16) else {
                break;
            };
            acc = (acc << 4) + d;
            if off > 3 {
                //more than 4 digits in group, fail.
                return Err(addr_err(input, "each group must have 4 or less digits", s));
            }
            if acc > u32::from(u16::MAX) {
                // Overflow, fail.
                return Err(addr_err(input, "IPv6 field has value >=2^16", s));
            }
            off += 1;
        }
        if off == 0 {
            // No digits found, fail.
            return Err(addr_err(
                input,
                "each colon-separated field must have at least one digit",
                s,
            ));
        }

        // If followed by dot, might be in trailing IPv4.
        if off < s.len() && s[off] == b'.' {
            if ellipsis < 0 && i != 12 {
                // Not the right place.
                return Err(addr_err(
                    input,
                    "embedded IPv4 address must replace the final 2 fields of the address",
                    s,
                ));
            }
            if i + 4 > 16 {
                // Not enough room.
                return Err(addr_err(
                    input,
                    "too many hex fields to fit an embedded IPv4 at the end of the address",
                    s,
                ));
            }
            let mut end = input.len();
            if !zone.is_empty() {
                end -= zone.len() + 1;
            }
            parse_ipv4_fields(input, end - s.len(), end)?;
            s = b"";
            i += 4;
            break;
        }

        // Save this 16-bit chunk.
        i += 2;

        // Stop at end of string.
        s = &s[off..];
        if s.is_empty() {
            break;
        }

        // Otherwise must be followed by colon and more.
        if s[0] != b':' {
            return Err(addr_err(input, "unexpected character, want colon", s));
        } else if s.len() == 1 {
            return Err(addr_err(
                input,
                "colon must be followed by more characters",
                s,
            ));
        }
        s = &s[1..];

        // Look for ellipsis.
        if s[0] == b':' {
            if ellipsis >= 0 {
                // already have one
                return Err(addr_err(input, "multiple :: in address", s));
            }
            ellipsis = i;
            s = &s[1..];
            if s.is_empty() {
                // can be at end
                break;
            }
        }
    }

    // Must have used entire string.
    if !s.is_empty() {
        return Err(addr_err(input, "trailing garbage after address", s));
    }

    // If didn't parse enough, expand ellipsis.
    if i < 16 {
        if ellipsis < 0 {
            return Err(addr_err(input, "address string too short", b""));
        }
    } else if ellipsis >= 0 {
        // Ellipsis must represent at least one 0 group.
        return Err(addr_err(
            input,
            "the :: must expand to at least one field of zeros",
            b"",
        ));
    }
    ok(zone)
}

// ---- url.Parse ----

/// Url is what the checks need from a parsed url.URL.
struct Url {
    scheme: String,
    host: Vec<u8>,
    raw_path: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Enc {
    Path,
    PathSegment,
    Host,
    Zone,
    UserPassword,
    Fragment,
}

/// should_escape is url.shouldEscape.
fn should_escape(c: u8, mode: Enc) -> bool {
    // §2.3 Unreserved characters (alphanum)
    if c.is_ascii_alphanumeric() {
        return false;
    }
    if (mode == Enc::Host || mode == Enc::Zone)
        && matches!(
            c,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'['
                | b']'
                | b'<'
                | b'>'
                | b'"'
        )
    {
        return false;
    }
    match c {
        // §2.3 Unreserved characters (mark)
        b'-' | b'_' | b'.' | b'~' => return false,
        // §2.2 Reserved characters (reserved)
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => match mode {
            Enc::Path => return c == b'?',
            Enc::PathSegment => return matches!(c, b'/' | b';' | b',' | b'?'),
            Enc::UserPassword => return matches!(c, b'@' | b'/' | b'?' | b':'),
            Enc::Fragment => return false,
            Enc::Host | Enc::Zone => {}
        },
        _ => {}
    }
    if mode == Enc::Fragment && matches!(c, b'!' | b'(' | b')' | b'*') {
        return false;
    }
    // Everything else must be escaped.
    true
}

fn unhex(c: u8) -> u8 {
    char::from(c).to_digit(16).unwrap_or(0) as u8
}

/// unescape is url.unescape with its EscapeError and InvalidHostError texts.
fn unescape(s: &[u8], mode: Enc) -> Result<Vec<u8>, String> {
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                if i + 2 >= s.len()
                    || !s[i + 1].is_ascii_hexdigit()
                    || !s[i + 2].is_ascii_hexdigit()
                {
                    let e = &s[i..(i + 3).min(s.len())];
                    return Err(format!("invalid URL escape {}", go_quote_bytes(e)));
                }
                // in the host component %-encoding can only be used for non-ASCII bytes
                if mode == Enc::Host && unhex(s[i + 1]) < 8 && &s[i..i + 3] != b"%25" {
                    return Err(format!(
                        "invalid URL escape {}",
                        go_quote_bytes(&s[i..i + 3])
                    ));
                }
                if mode == Enc::Zone {
                    let v = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                    if &s[i..i + 3] != b"%25" && v != b' ' && should_escape(v, Enc::Host) {
                        return Err(format!(
                            "invalid URL escape {}",
                            go_quote_bytes(&s[i..i + 3])
                        ));
                    }
                }
                i += 3;
            }
            c => {
                if (mode == Enc::Host || mode == Enc::Zone) && c < 0x80 && should_escape(c, mode) {
                    return Err(format!(
                        "invalid character {} in host name",
                        go_quote_bytes(&s[i..i + 1])
                    ));
                }
                i += 1;
            }
        }
    }
    let mut t = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            t.push((unhex(s[i + 1]) << 4) | unhex(s[i + 2]));
            i += 3;
        } else {
            t.push(s[i]);
            i += 1;
        }
    }
    Ok(t)
}

/// escape is url.escape for the path.
fn escape_path(s: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(s.len());
    for &c in s {
        if should_escape(c, Enc::Path) {
            t.extend_from_slice(format!("%{c:02X}").as_bytes());
        } else {
            t.push(c);
        }
    }
    t
}

fn valid_optional_port(port: &[u8]) -> bool {
    if port.is_empty() {
        return true;
    }
    port[0] == b':' && port[1..].iter().all(u8::is_ascii_digit)
}

fn valid_userinfo(s: &[u8]) -> bool {
    lossy(s).chars().all(|r| {
        r.is_ascii_alphanumeric()
            || matches!(
                r,
                '-' | '.'
                    | '_'
                    | ':'
                    | '~'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '%'
                    | '@'
            )
    })
}

fn rfind(s: &[u8], c: u8) -> Option<usize> {
    s.iter().rposition(|&x| x == c)
}

fn find(s: &[u8], c: u8) -> Option<usize> {
    s.iter().position(|&x| x == c)
}

/// parse_host is url.parseHost.
fn parse_host(scheme: &str, host: &[u8]) -> Result<Vec<u8>, String> {
    match rfind(host, b'[') {
        Some(i) if i > 0 => return Err("invalid IP-literal".into()),
        Some(_) => {
            // Parse an IP-Literal in RFC 3986 and RFC 6874.
            let Some(close) = rfind(host, b']') else {
                return Err("missing ']' in host".into());
            };
            let colon_port = &host[close + 1..];
            if !valid_optional_port(colon_port) {
                return Err(format!(
                    "invalid port {} after host",
                    go_quote_bytes(colon_port)
                ));
            }
            let unescaped_colon_port = unescape(colon_port, Enc::Host)?;
            let hostname = &host[1..close.max(1)];
            let zone_idx = hostname.windows(3).position(|w| w == b"%25");
            let unescaped_hostname = match zone_idx {
                Some(z) => {
                    let mut h = unescape(&hostname[..z], Enc::Host)?;
                    h.extend(unescape(&hostname[z..], Enc::Zone)?);
                    h
                }
                None => unescape(hostname, Enc::Host)?,
            };
            // Per RFC 3986, only a host identified by a valid IPv6 address
            // can be enclosed by square brackets.
            let addr = parse_addr(&unescaped_hostname).map_err(|e| format!("invalid host: {e}"))?;
            if addr.is4 {
                return Err("invalid IP-literal".into());
            }
            let mut out = b"[".to_vec();
            out.extend(unescaped_hostname);
            out.push(b']');
            out.extend(unescaped_colon_port);
            return Ok(out);
        }
        None => {
            if let Some(mut i) = find(host, b':') {
                let last_colon = rfind(host, b':').unwrap_or(i);
                // colons in the host are allowed for other schemes than
                // http and https (urlstrictcolons is on by default)
                if last_colon != i && scheme != "http" && scheme != "https" {
                    i = last_colon;
                }
                let colon_port = &host[i..];
                if !valid_optional_port(colon_port) {
                    return Err(format!(
                        "invalid port {} after host",
                        go_quote_bytes(colon_port)
                    ));
                }
            }
        }
    }
    unescape(host, Enc::Host)
}

/// parse_url is the library's parseURL: url.Parse, then a check of an IPv6
/// host.
fn parse_url(s: &str) -> Result<Url, String> {
    let u = url_parse(s.as_bytes())?;

    // gourl does not validate ipv6 host address
    let host_name = hostname(&u.host);
    if host_name.contains(&b':') {
        if !u.host.contains(&b'[') || !u.host.contains(&b']') {
            return Err("ipv6 address not enclosed in brackets".into());
        }
        if let Err(e) = validate_ipv6(&host_name) {
            return Err(format!("invalid ipv6 address: {e}"));
        }
    }
    Ok(u)
}

/// hostname is url.URL.Hostname.
fn hostname(host: &[u8]) -> Vec<u8> {
    let mut h = host;
    if let Some(colon) = rfind(h, b':')
        && valid_optional_port(&h[colon..])
    {
        h = &h[..colon];
    }
    if h.len() >= 2 && h[0] == b'[' && h[h.len() - 1] == b']' {
        h = &h[1..h.len() - 1];
    }
    h.to_vec()
}

/// url_parse is url.Parse with its errors.
fn url_parse(raw: &[u8]) -> Result<Url, String> {
    // Cut off #frag
    let (u, frag) = match find(raw, b'#') {
        Some(i) => (&raw[..i], &raw[i + 1..]),
        None => (raw, &b""[..]),
    };
    let url = parse(u).map_err(|e| format!("parse {}: {e}", go_quote_bytes(u)))?;
    if frag.is_empty() {
        return Ok(url);
    }
    unescape(frag, Enc::Fragment).map_err(|e| format!("parse {}: {e}", go_quote_bytes(raw)))?;
    Ok(url)
}

/// parse is url.parse with viaRequest false.
fn parse(raw: &[u8]) -> Result<Url, String> {
    if raw.iter().any(|&b| b < b' ' || b == 0x7f) {
        return Err("net/url: invalid control character in URL".into());
    }
    let mut url = Url {
        scheme: String::new(),
        host: Vec::new(),
        raw_path: Vec::new(),
    };
    if raw == b"*" {
        return Ok(url);
    }

    // Split off possible leading "http:", "mailto:", etc.
    let (scheme, mut rest) = get_scheme(raw)?;
    url.scheme = String::from_utf8_lossy(scheme).to_ascii_lowercase();

    if rest.ends_with(b"?") && rest.iter().filter(|&&c| c == b'?').count() == 1 {
        rest = &rest[..rest.len() - 1];
    } else if let Some(i) = find(rest, b'?') {
        rest = &rest[..i];
    }

    if !rest.starts_with(b"/") {
        if !url.scheme.is_empty() {
            // We consider rootless paths per RFC 3986 as opaque.
            return Ok(url);
        }
        // a relative-path reference: the first path segment cannot contain a colon
        let segment = &rest[..find(rest, b'/').unwrap_or(rest.len())];
        if segment.contains(&b':') {
            return Err("first path segment in URL cannot contain colon".into());
        }
    }

    if (!url.scheme.is_empty() || !rest.starts_with(b"///")) && rest.starts_with(b"//") {
        let mut authority = &rest[2..];
        rest = b"";
        if let Some(i) = find(authority, b'/') {
            (authority, rest) = (&authority[..i], &authority[i..]);
        }
        url.host = parse_authority(&url.scheme, authority)?;
    }

    // Set Path and, optionally, RawPath.
    let path = unescape(rest, Enc::Path)?;
    if escape_path(&path) != rest {
        url.raw_path = rest.to_vec();
    }
    Ok(url)
}

fn get_scheme(raw: &[u8]) -> Result<(&[u8], &[u8]), String> {
    for (i, &c) in raw.iter().enumerate() {
        match c {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if i == 0 {
                    return Ok((b"", raw));
                }
            }
            b':' => {
                if i == 0 {
                    return Err("missing protocol scheme".into());
                }
                return Ok((&raw[..i], &raw[i + 1..]));
            }
            // an invalid character, so there is no valid scheme
            _ => return Ok((b"", raw)),
        }
    }
    Ok((b"", raw))
}

fn parse_authority(scheme: &str, authority: &[u8]) -> Result<Vec<u8>, String> {
    let at = rfind(authority, b'@');
    let host = parse_host(scheme, &authority[at.map_or(0, |i| i + 1)..])?;
    let Some(i) = at else {
        return Ok(host);
    };
    let userinfo = &authority[..i];
    if !valid_userinfo(userinfo) {
        return Err("net/url: invalid userinfo".into());
    }
    match find(userinfo, b':') {
        None => {
            unescape(userinfo, Enc::UserPassword)?;
        }
        Some(c) => {
            unescape(&userinfo[..c], Enc::UserPassword)?;
            unescape(&userinfo[c + 1..], Enc::UserPassword)?;
        }
    }
    Ok(host)
}
