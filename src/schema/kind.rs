use std::collections::{BTreeSet, HashMap, HashSet};

use jsonschema::Validator;
use serde_json::Value;

use super::compile::{go_join, written};
use super::texts::go_quote;
use super::{BUILTIN_FIELDS, Kind, MemLoader, Set, Violation, pointer};
use crate::loader::{self, Node};

impl Kind {
    pub(super) fn new(sc: Validator, root: &Node, u: &str, l: &MemLoader) -> Result<Kind, String> {
        let mut k = Kind {
            schema: sc,
            url: u.to_string(),
            closed: root
                .get("additionalProperties")
                .is_some_and(|n| n.str == "false"),
            allowed: BTreeSet::new(),
            targets: HashMap::new(),
            covered: false,
            covered_fields: BTreeSet::new(),
        };
        if let Some(c) = root.get("x-docc-covered") {
            if c.kind != loader::Kind::Bool {
                return Err(r#""x-docc-covered" must be true or false"#.to_string());
            }
            k.covered = c.str == "true";
        }
        let mut seen = HashSet::from([format!("{u}#")]);
        k.walk(l.docs.get(u).unwrap_or(&Value::Null), u, l, &mut seen)?;
        Ok(k)
    }

    /// walk collects the top-level names a kind allows and their x-docc-target:
    /// "properties" of the schema, of its "allOf" branches and of local "$ref"
    /// targets, recursively. Properties under if/then are not counted.
    fn walk(
        &mut self,
        v: &Value,
        base: &str,
        l: &MemLoader,
        seen: &mut HashSet<String>,
    ) -> Result<(), String> {
        let Some(m) = v.as_object() else {
            return Ok(());
        };
        if let Some(Value::Object(props)) = m.get("properties") {
            // the map iterates its keys sorted
            for (name, pm) in props {
                self.allowed.insert(name.clone());
                match pm.get("x-docc-covered") {
                    None => {}
                    Some(Value::Bool(c)) => {
                        if *c {
                            self.covered_fields.insert(name.clone());
                        }
                    }
                    Some(_) => {
                        return Err(format!(r#""x-docc-covered" of property {} must be true or false"#, go_quote(name)));
                    }
                }
                let Some(t) = pm.get("x-docc-target") else {
                    continue;
                };
                let bad = || {
                    format!(
                        r#""x-docc-target" of property {} must be a non-empty array of kind names"#,
                        go_quote(name)
                    )
                };
                let list = t.as_array().filter(|a| !a.is_empty()).ok_or_else(bad)?;
                for x in list {
                    let s = x.as_str().ok_or_else(bad)?;
                    let targets = self.targets.entry(name.clone()).or_default();
                    if !targets.iter().any(|t| t == s) {
                        targets.push(s.to_string());
                    }
                }
            }
        }
        if let Some(r) = written(m, "$ref")
            && let Ok((u, frag)) = go_join(base, r)
            && seen.insert(format!("{u}#{frag}"))
            && let Some(t) = pointer_in(l.docs.get(&u), &frag)
        {
            self.walk(t, &u, l, seen)?;
        }
        if let Some(Value::Array(branches)) = m.get("allOf") {
            for b in branches {
                self.walk(b, base, l, seen)?;
            }
        }
        Ok(())
    }

    /// Targets returns the kinds that references in the field must point to
    /// ("x-docc-target"); empty means any kind.
    pub fn targets(&self, field: &str) -> &[String] {
        self.targets
            .get(field)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// unknownFields reports the fields of a closed kind that the schema does not
    /// allow; the hint names the closest allowed name.
    pub(super) fn unknown_fields(&self, root: &Node) -> Vec<Violation> {
        if !self.closed {
            return Vec::new();
        }
        let mut out = Vec::new();
        for m in &root.members {
            if self.allowed.contains(&m.key)
                || BUILTIN_FIELDS.contains(&m.key.as_str())
                || m.key == "docIgnore"
                || m.key == crate::comments::KEY
            {
                continue;
            }
            let mut hint = format!(
                "Remove the field {} or add it to \"properties\" of the kind schema.",
                go_quote(&m.key)
            );
            let c = self.closest(&m.key);
            if !c.is_empty() {
                hint = format!("Did you mean {}? {hint}", go_quote(&c));
            }
            out.push(Violation {
                pointer: pointer(&[&m.key]),
                message: format!("unknown field {}", go_quote(&m.key)),
                hint,
                start: m.key_start,
                end: m.key_end,
            });
        }
        out
    }

    fn closest(&self, key: &str) -> String {
        let names: Vec<&str> = self
            .allowed
            .iter()
            .map(String::as_str)
            .chain(BUILTIN_FIELDS)
            .collect();
        closest(key, &names).0
    }
}

/// pointerIn resolves a JSON pointer fragment ("" or "/a/b") in a raw value.
pub(super) fn pointer_in<'a>(v: Option<&'a Value>, frag: &str) -> Option<&'a Value> {
    let mut v = v?;
    if frag.is_empty() {
        return Some(v);
    }
    for t in frag.strip_prefix('/')?.split('/') {
        let t = t.replace("~1", "/").replace("~0", "~");
        v = v.as_object()?.get(&t)?;
    }
    Some(v)
}

impl Set {
    /// Kind returns the kind schema, or None when there is none.
    pub fn kind(&self, name: &str) -> Option<&Kind> {
        self.kinds.get(name)
    }

    /// Declares reports whether the schema of the kind lists key as a property.
    pub fn declares(&self, kind_name: &str, key: &str) -> bool {
        self.kinds
            .get(kind_name)
            .is_some_and(|k| k.allowed.contains(key))
    }
}

/// Closest returns the name nearest to key and its edit distance; "" when none
/// is near enough to be a likely typo.
pub fn closest<S: AsRef<str>>(key: &str, names: &[S]) -> (String, usize) {
    let (mut best, mut best_d) = ("", 0);
    for n in names {
        let d = distance(key, n.as_ref());
        if best.is_empty() || d < best_d {
            (best, best_d) = (n.as_ref(), d);
        }
    }
    if best_d > 2 && best_d * 3 > key.chars().count() {
        return (String::new(), 0);
    }
    (best.to_string(), best_d)
}

/// distance is the Levenshtein distance between a and b.
fn distance(a: &str, b: &str) -> usize {
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=y.len()).collect();
    for i in 1..=x.len() {
        let mut cur = vec![0; y.len() + 1];
        cur[0] = i;
        for j in 1..=y.len() {
            let cost = usize::from(x[i - 1] != y[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[y.len()]
}
