use regex::Regex;

/// Glob is a compiled path mask: `*` does not cross '/', `**` does.
/// `**/` also matches zero directories.
#[derive(Debug, Clone)]
pub struct Glob {
    re: Regex,
}

pub fn compile_glob(mask: &str) -> Result<Glob, regex::Error> {
    let mut b = String::from("^");
    let mut i = 0;
    while i < mask.len() {
        let rest = &mask[i..];
        if rest.starts_with("**/") {
            b.push_str("(?:.*/)?");
            i += 3;
        } else if rest.starts_with("**") {
            b.push_str(".*");
            i += 2;
        } else if rest.starts_with('*') {
            b.push_str("[^/]*");
            i += 1;
        } else if rest.starts_with('?') {
            b.push_str("[^/]");
            i += 1;
        } else {
            // Go quotes one byte at a time; the bytes of a rune join up again.
            let c = rest.chars().next().unwrap();
            b.push_str(&regex::escape(c.encode_utf8(&mut [0; 4])));
            i += c.len_utf8();
        }
    }
    b.push('$');
    Ok(Glob {
        re: Regex::new(&b)?,
    })
}

impl Glob {
    pub fn matches(&self, path: &str) -> bool {
        self.re.is_match(path)
    }
}

/// MatchAny reports whether path matches at least one glob.
pub fn match_any(gs: &[Glob], path: &str) -> bool {
    gs.iter().any(|g| g.matches(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob() {
        let cases = [
            ("drafts/**", "drafts/a.json", true),
            ("drafts/**", "drafts/x/y.json", true),
            ("drafts/*", "drafts/x/y.json", false),
            ("*.json", "a.json", true),
            ("*.json", "d/a.json", false),
            ("**/a.json", "a.json", true),
            ("**/a.json", "x/y/a.json", true),
            ("a.json", "ab.json", false),
            ("tests/**", "tests/unit/a_test.go", true),
        ];
        for (mask, path, want) in cases {
            let g = compile_glob(mask).unwrap();
            assert_eq!(g.matches(path), want, "{mask:?} vs {path:?}");
        }
    }
}
