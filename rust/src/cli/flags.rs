//! A copy of the parts of Go's flag package that users of docc see: -x and
//! --x, -x=v and -x v, boolean flags, "--", the error texts, -h and -help, and
//! the flag list of PrintDefaults. The values live in the caller's variables,
//! as with flag.StringVar, so a value parsed before an error stays set.

use std::fmt;

/// Value points at the variable a flag sets.
enum Value<'a> {
    Str(&'a mut String),
    Int(&'a mut i64),
    Bool(&'a mut bool),
}

struct Flag<'a> {
    name: &'static str,
    usage: &'static str,
    /// the default as text (Go's DefValue)
    def: String,
    value: Value<'a>,
}

/// FlagError is what Parse fails with.
#[derive(Debug, PartialEq, Eq)]
pub enum FlagError {
    /// -h or -help was given and is not a defined flag (Go's flag.ErrHelp)
    Help,
    /// a parse error with Go's text
    Msg(String),
}

impl fmt::Display for FlagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlagError::Help => f.write_str("flag: help requested"),
            FlagError::Msg(m) => f.write_str(m),
        }
    }
}

/// FlagSet is flag.FlagSet with ContinueOnError and the output discarded.
pub struct FlagSet<'a> {
    name: String,
    flags: Vec<Flag<'a>>,
    args: Vec<String>,
}

impl<'a> FlagSet<'a> {
    pub fn new(name: &str) -> FlagSet<'a> {
        FlagSet { name: name.to_string(), flags: Vec::new(), args: Vec::new() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// StringVar: the current value of p is the default.
    pub fn string_var(&mut self, p: &'a mut String, name: &'static str, usage: &'static str) {
        let def = p.clone();
        self.flags.push(Flag { name, usage, def, value: Value::Str(p) });
    }

    /// IntVar: the current value of p is the default.
    pub fn int_var(&mut self, p: &'a mut i64, name: &'static str, usage: &'static str) {
        let def = p.to_string();
        self.flags.push(Flag { name, usage, def, value: Value::Int(p) });
    }

    /// BoolVar: the current value of p is the default.
    pub fn bool_var(&mut self, p: &'a mut bool, name: &'static str, usage: &'static str) {
        let def = p.to_string();
        self.flags.push(Flag { name, usage, def, value: Value::Bool(p) });
    }

    /// Args returns the arguments left after the flags.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Parse parses flags from args, which must not include the command name,
    /// and stops at the first argument that is not a flag.
    pub fn parse(&mut self, args: &[String]) -> Result<(), FlagError> {
        self.args = args.to_vec();
        while self.parse_one()? {}
        Ok(())
    }

    /// parse_one parses one flag; false when the flags end.
    fn parse_one(&mut self) -> Result<bool, FlagError> {
        let Some(s) = self.args.first().cloned() else {
            return Ok(false);
        };
        let b = s.as_bytes();
        if b.len() < 2 || b[0] != b'-' {
            return Ok(false);
        }
        let mut minuses = 1;
        if b[1] == b'-' {
            minuses += 1;
            if b.len() == 2 {
                // "--" terminates the flags
                self.args.remove(0);
                return Ok(false);
            }
        }
        let mut name = &s[minuses..];
        if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
            return Err(FlagError::Msg(format!("bad flag syntax: {s}")));
        }
        // it's a flag. does it have an argument?
        self.args.remove(0);
        let mut value: Option<&str> = None;
        // bytes, as Go: a name may start with a character of several bytes
        if let Some(i) = name.bytes().skip(1).position(|c| c == b'=') {
            // equals cannot be first
            value = Some(&name[i + 2..]);
            name = &name[..i + 1];
        }
        let Some(k) = self.flags.iter().position(|f| f.name == name) else {
            if name == "help" || name == "h" {
                // special case for nice help message
                return Err(FlagError::Help);
            }
            return Err(FlagError::Msg(format!("flag provided but not defined: -{name}")));
        };
        let next = if value.is_none() && !self.args.is_empty() && !matches!(self.flags[k].value, Value::Bool(_)) {
            // the value is the next argument
            Some(self.args.remove(0))
        } else {
            None
        };
        let value = value.map(str::to_string).or(next);
        match &mut self.flags[k].value {
            Value::Bool(p) => match value {
                None => **p = true,
                Some(v) => match parse_bool(&v) {
                    Some(x) => **p = x,
                    None => {
                        return Err(FlagError::Msg(format!(
                            "invalid boolean value {} for -{name}: parse error",
                            go_quote(&v)
                        )));
                    }
                },
            },
            Value::Str(p) => match value {
                Some(v) => **p = v,
                None => return Err(FlagError::Msg(format!("flag needs an argument: -{name}"))),
            },
            Value::Int(p) => match value {
                Some(v) => match parse_int(&v) {
                    Ok(x) => **p = x,
                    Err(e) => {
                        return Err(FlagError::Msg(format!(
                            "invalid value {} for flag -{name}: {e}",
                            go_quote(&v)
                        )));
                    }
                },
                None => return Err(FlagError::Msg(format!("flag needs an argument: -{name}"))),
            },
        }
        Ok(true)
    }

    /// PrintDefaults returns what flag.PrintDefaults prints: every flag in
    /// lexical order with its type, usage and a default that is not the zero value.
    pub fn defaults(&self) -> String {
        let mut fl: Vec<&Flag> = self.flags.iter().collect();
        fl.sort_by_key(|f| f.name);
        let mut out = String::new();
        for f in fl {
            let mut b = format!("  -{}", f.name);
            let (kind, zero) = match f.value {
                Value::Str(_) => ("string", ""),
                Value::Int(_) => ("int", "0"),
                Value::Bool(_) => ("", "false"),
            };
            if !kind.is_empty() {
                b.push(' ');
                b.push_str(kind);
            }
            // Boolean flags of one ASCII letter are so common we
            // treat them specially, putting their usage on the same line.
            if b.len() <= 4 {
                b.push('\t');
            } else {
                // Four spaces before the tab triggers good alignment
                // for both 4- and 8-space tab stops.
                b.push_str("\n    \t");
            }
            b.push_str(&f.usage.replace('\n', "\n    \t"));
            if f.def != zero {
                if matches!(f.value, Value::Str(_)) {
                    b.push_str(&format!(" (default {})", go_quote(&f.def)));
                } else {
                    b.push_str(&format!(" (default {})", f.def));
                }
            }
            out.push_str(&b);
            out.push('\n');
        }
        out
    }
}

/// go_quote is Go's %q of a flag value.
fn go_quote(s: &str) -> String {
    crate::config::go_quote(s)
}

/// parse_bool is strconv.ParseBool.
fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// parse_int is strconv.ParseInt(s, 0, 64); the error is the text the flag
/// package puts in its message.
fn parse_int(s0: &str) -> Result<i64, &'static str> {
    const SYNTAX: &str = "parse error";
    const RANGE: &str = "value out of range";
    let mut s = s0.as_bytes();
    if s.is_empty() {
        return Err(SYNTAX);
    }
    let neg = s[0] == b'-';
    if s[0] == b'+' || s[0] == b'-' {
        s = &s[1..];
    }
    // ParseUint with base 0
    if s.is_empty() {
        return Err(SYNTAX);
    }
    let unsigned = s;
    let lower = |c: u8| c | 0x20;
    let mut base: u64 = 10;
    if s[0] == b'0' {
        if s.len() >= 3 && lower(s[1]) == b'b' {
            (base, s) = (2, &s[2..]);
        } else if s.len() >= 3 && lower(s[1]) == b'o' {
            (base, s) = (8, &s[2..]);
        } else if s.len() >= 3 && lower(s[1]) == b'x' {
            (base, s) = (16, &s[2..]);
        } else {
            (base, s) = (8, &s[1..]);
        }
    }
    let cutoff = u64::MAX / base + 1;
    let mut n: u64 = 0;
    let mut underscores = false;
    for &c in s {
        let d = match c {
            b'_' => {
                underscores = true;
                continue;
            }
            b'0'..=b'9' => c - b'0',
            _ if lower(c).is_ascii_lowercase() => lower(c) - b'a' + 10,
            _ => return Err(SYNTAX),
        } as u64;
        if d >= base {
            return Err(SYNTAX);
        }
        if n >= cutoff {
            return Err(RANGE);
        }
        n *= base;
        let n1 = n.wrapping_add(d);
        if n1 < n {
            return Err(RANGE);
        }
        n = n1;
    }
    if underscores && !underscore_ok(unsigned) {
        return Err(SYNTAX);
    }
    let cut = 1u64 << 63;
    if !neg && n >= cut {
        return Err(RANGE);
    }
    if neg && n > cut {
        return Err(RANGE);
    }
    Ok(if neg { (n as i64).wrapping_neg() } else { n as i64 })
}

/// underscore_ok is strconv.underscoreOK: underscores only between digits
/// or after a base prefix.
fn underscore_ok(mut s: &[u8]) -> bool {
    // saw: b'^' beginning, b'0' digit or base prefix, b'_' underscore, b'!' other
    let mut saw = b'^';
    let mut i = 0;
    if !s.is_empty() && (s[0] == b'-' || s[0] == b'+') {
        s = &s[1..];
    }
    let lower = |c: u8| c | 0x20;
    let mut hex = false;
    if s.len() >= 2 && s[0] == b'0' && matches!(lower(s[1]), b'b' | b'o' | b'x') {
        i = 2;
        saw = b'0';
        hex = lower(s[1]) == b'x';
    }
    while i < s.len() {
        let c = s[i];
        i += 1;
        if c.is_ascii_digit() || hex && (b'a'..=b'f').contains(&lower(c)) {
            saw = b'0';
            continue;
        }
        if c == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
            continue;
        }
        if saw == b'_' {
            return false;
        }
        saw = b'!';
    }
    saw != b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_like_go() {
        let (mut f, mut n, mut b) = (String::from("text"), 20i64, false);
        let mut fs = FlagSet::new("x");
        fs.string_var(&mut f, "format", "text or json");
        fs.int_var(&mut n, "max", "limit");
        fs.bool_var(&mut b, "full", "full");
        assert_eq!(fs.parse(&args(&["--format=json", "-max", "0x10", "--full", "a", "-full=false"])), Ok(()));
        assert_eq!(fs.args(), args(&["a", "-full=false"]));
        assert_eq!(fs.parse(&args(&["-max", "1_000", "--", "-x"])), Ok(()));
        assert_eq!(fs.args(), args(&["-x"]));
        for (a, want) in [
            (vec!["-bogus"], "flag provided but not defined: -bogus"),
            (vec!["--format"], "flag needs an argument: -format"),
            (vec!["-max", "x"], "invalid value \"x\" for flag -max: parse error"),
            (vec!["-max", "99999999999999999999"], "invalid value \"99999999999999999999\" for flag -max: value out of range"),
            (vec!["-max", "1__0"], "invalid value \"1__0\" for flag -max: parse error"),
            (vec!["-full=maybe"], "invalid boolean value \"maybe\" for -full: parse error"),
            (vec!["---x"], "bad flag syntax: ---x"),
            (vec!["-=x"], "bad flag syntax: -=x"),
            // a name that starts with a character of several bytes (the Russian layout)
            (vec!["--ащкьфе", "json"], "flag provided but not defined: -ащкьфе"),
            (vec!["-é=1"], "flag provided but not defined: -é"),
        ] {
            assert_eq!(fs.parse(&args(&a)), Err(FlagError::Msg(want.into())), "{a:?}");
        }
        assert_eq!(fs.parse(&args(&["-h"])), Err(FlagError::Help));
        assert_eq!(fs.parse(&args(&["--help=1"])), Err(FlagError::Help));
        assert_eq!(fs.parse(&args(&["-max", "-0b11"])), Ok(()));
        assert_eq!(fs.parse(&args(&["-max", "-9223372036854775808"])), Ok(()));
        assert_eq!(
            fs.defaults(),
            "  -format string\n    \ttext or json (default \"text\")\n  -full\n    \tfull\n  -max int\n    \tlimit (default 20)\n"
        );
        drop(fs);
        assert_eq!((f.as_str(), n, b), ("json", i64::MIN, true));
    }
}
