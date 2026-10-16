//! What the Go library (santhosh-tekuri/jsonschema/v6) checks when it
//! compiles a schema, and the rewriting that lets the Rust validator read a
//! schema the way the Go library does.
//!
//! The Go compiler is lazy: it starts at the root of a file, loads a resource
//! only when a reachable "$ref" points into it, checks every loaded resource
//! against its metaschema and resolves references with net/url. The Rust
//! validator resolves every reference of a document up front and needs
//! RFC 3986 URIs. So go_compile walks the schema as the Go compiler does and
//! reports its errors, and the Rust validator gets URIs made valid by
//! crate_safe and a stand-in for a resource that does not exist (the walk has
//! already reported every one that a schema can reach).

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::texts::{go_quote, quote};
use super::{MemLoader, pointer};
use crate::config as cfg;

/// go_compile reports the first error the Go compiler meets compiling the
/// schema at input: a reference it cannot load or resolve, a resource its
/// metaschema rejects, a URL net/url cannot parse.
pub(super) fn go_compile(l: &MemLoader, input: &str) -> Result<(), String> {
    let mut w = Walk {
        l,
        roots: HashMap::new(),
        queue: Vec::new(),
        seen: HashSet::new(),
    };
    let (u, frag) = absolute(input)?;
    w.load(&u)?;
    let ptr = w.fragment(&u, "", &frag)?;
    w.push(&u, ptr);
    let mut i = 0;
    while i < w.queue.len() {
        let (u, ptr) = w.queue[i].clone();
        w.compile(&u, &ptr)?;
        i += 1;
    }
    Ok(())
}

/// SchemaAt is a schema by document URL and JSON pointer.
pub(super) type SchemaAt = (String, String);

/// CYCLE_URL starts the URL of the stand-in break_cycles puts in place of a
/// reference that closes a cycle.
pub(super) const CYCLE_URL: &str = "docc://cycle/";

/// break_cycles finds the cycles of subschemas applied at one place of the
/// document ("$ref" to itself, "allOf" back to its parent and the like). The
/// Go validator stops when it meets a schema a second time at one place and
/// reports a reference cycle; the Rust validator takes a cycle as valid. So
/// one reference of every cycle is pointed at a false schema, and
/// cycle_text words the error it gives as the Go library does. The
/// reference keeps what it was written as (written).
pub(super) fn break_cycles(l: &mut MemLoader) {
    let mut urls: Vec<&String> = l.docs.keys().collect();
    urls.sort();
    let mut nodes: Vec<SchemaAt> = Vec::new();
    for u in urls {
        all_schemas(&l.docs[u], &mut String::new(), &mut |p| {
            nodes.push((u.clone(), p.to_string()))
        });
    }
    let mut edges: Edges = HashMap::new();
    let mut w = Walk {
        l: &*l,
        roots: HashMap::new(),
        queue: Vec::new(),
        seen: HashSet::new(),
    };
    for n in &nodes {
        let e = w.in_place(n);
        if !e.is_empty() {
            edges.insert(n.clone(), e);
        }
    }
    let mut n = 0;
    while let Some(cycle) = find_cycle(&nodes, &edges) {
        // the last reference of the cycle (one always closes it)
        let Some(i) = cycle.iter().rposition(|(_, (_, _, kw))| kw.is_some()) else {
            break;
        };
        let (from, (_, to, kw)) = cycle[i].clone();
        let kw = kw.unwrap_or("$ref");
        if let Some(list) = edges.get_mut(&from) {
            list.retain(|(_, t, k)| !(t == &to && *k == Some(kw)));
        }
        // from the target on, the way round the cycle
        let round: Round = cycle[i + 1..]
            .iter()
            .chain(&cycle[..=i])
            .map(|(_, (seg, t, _))| (seg.clone(), t.clone()))
            .collect();
        let stand_in = format!("{CYCLE_URL}{n}");
        n += 1;
        if let Some(Value::Object(m)) = l.docs.get_mut(&from.0).and_then(|d| lookup_mut(d, &from.1))
        {
            let orig = m.get(kw).cloned().unwrap_or_default();
            m.entry(format!("x-docc-go{kw}")).or_insert(orig);
            m.insert(kw.to_string(), Value::String(stand_in.clone()));
        }
        if kw != "$ref"
            && let Some(Value::Object(root)) = l.docs.get_mut(&from.0)
            && let Value::Object(defs) = root
                .entry("$defs")
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
        {
            // The Rust validator loads the documents "$ref" points to before
            // it validates, not those of "$dynamicRef": a "$ref" here makes it
            // load the stand-in.
            defs.insert(
                format!("x-docc-{stand_in}"),
                serde_json::json!({ "$ref": stand_in }),
            );
        }
        l.docs.insert(stand_in.clone(), Value::Bool(false));
        l.by_crate.insert(stand_in.clone(), stand_in.clone());
        l.cycles.insert(stand_in, round);
    }
}

/// all_schemas calls f with the pointer of every subschema of a schema.
fn all_schemas(v: &Value, at: &mut String, f: &mut impl FnMut(&str)) {
    f(at);
    let Some(m) = v.as_object() else {
        return;
    };
    let mut subs = Vec::new();
    each_subschema(m, |t, sub| subs.push((pointer(t), sub)));
    for (rel, sub) in subs {
        let n = at.len();
        at.push_str(&rel);
        all_schemas(sub, at, f);
        at.truncate(n);
    }
}

/// InPlace is a subschema applied at the same place: the keyword path to it,
/// where it is, and the reference keyword when a reference leads there.
type InPlace = (String, SchemaAt, Option<&'static str>);

type Edges = HashMap<SchemaAt, Vec<InPlace>>;

type Edge = (SchemaAt, InPlace);

/// Round is the way round a cycle: the keyword path to each next schema.
pub(super) type Round = Vec<(String, SchemaAt)>;

/// find_cycle returns the edges of a cycle of the graph, in order, if any.
fn find_cycle(nodes: &[SchemaAt], edges: &Edges) -> Option<Vec<Edge>> {
    // 1: on the path, 2: done
    let mut state: HashMap<SchemaAt, u8> = HashMap::new();
    let mut path: Vec<Edge> = Vec::new();
    fn visit(
        n: &SchemaAt,
        edges: &Edges,
        state: &mut HashMap<SchemaAt, u8>,
        path: &mut Vec<Edge>,
    ) -> Option<Vec<Edge>> {
        state.insert(n.clone(), 1);
        for e in edges.get(n).into_iter().flatten() {
            path.push((n.clone(), e.clone()));
            match state.get(&e.1) {
                Some(1) => {
                    let start = path.iter().position(|(from, _)| *from == e.1)?;
                    return Some(path[start..].to_vec());
                }
                Some(_) => {}
                None => {
                    if let Some(c) = visit(&e.1, edges, state, path) {
                        return Some(c);
                    }
                }
            }
            path.pop();
        }
        state.insert(n.clone(), 2);
        None
    }
    for n in nodes {
        if !state.contains_key(n)
            && let Some(c) = visit(n, edges, &mut state, &mut path)
        {
            return Some(c);
        }
    }
    None
}

/// cycle_text is the Go library's error for a document that reaches the
/// stand-in of a cycle: the validator started at start, whose keyword path
/// is kw_loc, and got to the stand-in by the keyword path eval. The Go
/// validator goes on round the cycle until it meets a schema it already
/// applied at this place: "both <path now> and <path then> resolve to
/// <schema> causing reference cycle".
pub(super) fn cycle_text(
    l: &MemLoader,
    start: &SchemaAt,
    kw_loc: &str,
    eval: &str,
    stand_in: &str,
) -> Option<String> {
    let round = l
        .cycles
        .get(stand_in.split('#').next().unwrap_or(stand_in))?;
    let mut w = Walk {
        l,
        roots: HashMap::new(),
        queue: Vec::new(),
        seen: HashSet::new(),
    };
    let toks: Vec<String> = eval
        .split('/')
        .skip(1)
        .map(|t| unescape_token(t).unwrap_or_default())
        .collect();
    let mut at = start.clone();
    let mut kw_loc = kw_loc.to_string();
    // the schemas applied at the current place of the document
    let mut here: Vec<(SchemaAt, String)> = vec![(at.clone(), kw_loc.clone())];
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();
        let m = l
            .docs
            .get(&at.0)
            .and_then(|d| lookup(d, &at.1).ok())
            .and_then(Value::as_object)?;
        if matches!(t, "$ref" | "$dynamicRef" | "$recursiveRef") {
            w.load(&at.0).ok()?;
            let (_, base) = w.resource(&at.0, &at.1);
            at = w.resolve(&at.0, &base, written(m, t)?).ok()?;
            kw_loc.push_str(&format!("/{t}"));
            here.push((at.clone(), kw_loc.clone()));
            i += 1;
            continue;
        }
        let two = matches!(
            t,
            "allOf"
                | "anyOf"
                | "oneOf"
                | "dependentSchemas"
                | "dependencies"
                | "properties"
                | "patternProperties"
                | "prefixItems"
        ) || (t == "items" && m.get("items").is_some_and(Value::is_array));
        let n = if two { 2 } else { 1 };
        let rel = pointer(toks.get(i..i + n)?);
        let doc = l.docs.get(&at.0)?;
        if lookup(doc, &format!("{}{rel}", at.1)).is_err() && at.1.ends_with(&rel) {
            // The Rust validator puts the end of the pointer of a reference
            // to a subschema ("#/$defs/x/allOf/0") after the "$ref" again.
            i += n;
            continue;
        }
        at = (at.0.clone(), format!("{}{rel}", at.1));
        kw_loc.push_str(&encode(&rel));
        if matches!(
            t,
            "allOf"
                | "anyOf"
                | "oneOf"
                | "not"
                | "if"
                | "then"
                | "else"
                | "dependentSchemas"
                | "dependencies"
        ) {
            here.push((at.clone(), kw_loc.clone()));
        } else {
            here = vec![(at.clone(), kw_loc.clone())];
        }
        i += n;
    }
    let found = |here: &[(SchemaAt, String)], s: &SchemaAt| {
        here.iter().find(|(h, _)| h == s).map(|(_, p)| p.clone())
    };
    let text = |now: &str, then: &str, s: &SchemaAt| {
        format!(
            "both {now} and {then} resolve to {} causing reference cycle",
            go_quote(&format!("{}#{}", s.0, encode(&s.1)))
        )
    };
    let (last, before) = here.split_last()?;
    if let Some(then) = found(before, &last.0) {
        return Some(text(&last.1, &then, &last.0));
    }
    let mut now = last.1.clone();
    let mut here = here.clone();
    for (seg, s) in round {
        now.push_str(seg);
        if let Some(then) = found(&here, s) {
            return Some(text(&now, &then, s));
        }
        here.push((s.clone(), now.clone()));
    }
    None
}

/// lookup_mut is lookup for changing the value.
fn lookup_mut<'a>(v: &'a mut Value, ptr: &str) -> Option<&'a mut Value> {
    let mut v = v;
    for t in ptr.split('/').skip(1) {
        let t = unescape_token(t)?;
        v = match v {
            Value::Object(m) => m.get_mut(&t)?,
            Value::Array(a) => a.get_mut(t.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(v)
}

/// Walk is the state of one go_compile: the queue of schemas to compile, as
/// in the Go compiler.
struct Walk<'a> {
    l: &'a MemLoader,
    /// loaded documents: url -> their resources as (pointer, id)
    roots: HashMap<String, Vec<(String, String)>>,
    queue: Vec<(String, String)>,
    seen: HashSet<(String, String)>,
}

impl Walk<'_> {
    /// load loads the document at u once: it must be a file of the schemas
    /// directory that its metaschema accepts.
    fn load(&mut self, u: &str) -> Result<(), String> {
        if self.roots.contains_key(u) {
            return Ok(());
        }
        let doc = self.l.load(u)?;
        let mut ids = Vec::new();
        collect_ids(doc, u, &mut Vec::new(), &mut ids);
        if let Some(e) = self.l.rejected.get(u) {
            return Err(e.clone());
        }
        self.roots.insert(u.to_string(), ids);
        Ok(())
    }

    fn push(&mut self, u: &str, ptr: String) {
        if self.seen.insert((u.to_string(), ptr.clone())) {
            self.queue.push((u.to_string(), ptr));
        }
    }

    /// resource returns the id of the resource ptr lies in: the nearest
    /// subschema with "$id", or the document.
    fn resource(&self, u: &str, ptr: &str) -> (String, String) {
        let mut best = (String::new(), u.to_string());
        for (p, id) in self.roots.get(u).into_iter().flatten() {
            let inside = p.is_empty() || ptr == p || ptr.starts_with(&format!("{p}/"));
            if inside && p.len() >= best.0.len() {
                best = (p.clone(), id.clone());
            }
        }
        best
    }

    /// compile looks the schema up and queues its subschemas in the order the
    /// Go compiler does; references are resolved (and their documents loaded)
    /// right away.
    fn compile(&mut self, u: &str, ptr: &str) -> Result<(), String> {
        let doc = self.l.load(u)?;
        let v = lookup(doc, ptr).map_err(|invalid| {
            let at = go_quote(&format!("{u}#{}", encode(ptr)));
            if invalid {
                format!("invalid json-pointer {at}")
            } else {
                format!("json-pointer in {at} not found")
            }
        })?;
        let Some(m) = v.as_object().filter(|m| !m.is_empty()) else {
            return Ok(());
        };
        let old = draft_before_2019(doc);
        let (_, base) = self.resource(u, ptr);
        let at = |kw: &str| format!("{ptr}{}", pointer(&[kw]));
        let mut todo: Vec<String> = Vec::new();
        if let Some(r) = written(m, "$ref") {
            self.reference(u, &base, r)?;
            if old {
                // before 2019-09 the keywords next to "$ref" are ignored
                return Ok(());
            }
        }
        for kw in ["allOf", "anyOf", "oneOf"] {
            if let Some(Value::Array(a)) = m.get(kw) {
                todo.extend((0..a.len()).map(|i| format!("{}/{i}", at(kw))));
            }
        }
        if m.contains_key("not") {
            todo.push(at("not"));
        }
        if let Some(Value::Array(a)) = m.get("items") {
            todo.extend((0..a.len()).map(|i| format!("{}/{i}", at("items"))));
            if m.contains_key("additionalItems") {
                todo.push(at("additionalItems"));
            }
        }
        for kw in ["properties", "patternProperties"] {
            if let Some(Value::Object(o)) = m.get(kw) {
                todo.extend(o.keys().map(|k| format!("{}{}", at(kw), pointer(&[k]))));
            }
        }
        if m.contains_key("additionalProperties") {
            todo.push(at("additionalProperties"));
        }
        if let Some(Value::Object(o)) = m.get("dependencies") {
            for (k, d) in o {
                if !d.is_array() {
                    todo.push(format!("{}{}", at("dependencies"), pointer(&[k])));
                }
            }
        }
        for kw in ["contains", "propertyNames"] {
            if m.contains_key(kw) {
                todo.push(at(kw));
            }
        }
        if let Some(cond) = m.get("if") {
            todo.push(at("if"));
            // a boolean "if" decides the branch already
            if cond != &Value::Bool(false) && m.contains_key("then") {
                todo.push(at("then"));
            }
            if cond != &Value::Bool(true) && m.contains_key("else") {
                todo.push(at("else"));
            }
        }
        if !old {
            if let Some(r) = written(m, "$recursiveRef") {
                self.reference(u, &base, r)?;
            }
            if let Some(Value::Object(o)) = m.get("dependentSchemas") {
                todo.extend(
                    o.keys()
                        .map(|k| format!("{}{}", at("dependentSchemas"), pointer(&[k]))),
                );
            }
            for kw in ["unevaluatedItems", "unevaluatedProperties"] {
                if m.contains_key(kw) {
                    todo.push(at(kw));
                }
            }
            if let Some(r) = written(m, "$dynamicRef") {
                self.reference(u, &base, r)?;
            }
            if let Some(Value::Array(a)) = m.get("prefixItems") {
                todo.extend((0..a.len()).map(|i| format!("{}/{i}", at("prefixItems"))));
            }
        }
        if m.get("items").is_some_and(|i| !i.is_array()) {
            todo.push(at("items"));
        }
        for p in todo {
            self.push(u, p);
        }
        Ok(())
    }

    /// reference resolves r against the resource id base of the document u
    /// and queues its target.
    fn reference(&mut self, u: &str, base: &str, r: &str) -> Result<(), String> {
        let (ru, ptr) = self.resolve(u, base, r)?;
        self.push(&ru, ptr);
        Ok(())
    }

    /// resolve resolves r against the resource id base of the document u,
    /// loading another document when it points there.
    fn resolve(&mut self, u: &str, base: &str, r: &str) -> Result<SchemaAt, String> {
        let (ru, frag) = go_join(base, r)?;
        let local = if ru == u {
            Some(String::new())
        } else {
            self.roots
                .get(u)
                .and_then(|ids| ids.iter().find(|(_, id)| *id == ru))
                .map(|(p, _)| p.clone())
        };
        if let Some(res) = local {
            return Ok((u.to_string(), self.fragment(u, &res, &frag)?));
        }
        self.load(&ru)?;
        let ptr = self.fragment(&ru, "", &frag)?;
        Ok((ru, ptr))
    }

    /// in_place lists the subschemas the Go validator applies at the same
    /// place of the document as the schema at s: the keyword path to each (as
    /// the Go library prints it), the subschema, and the reference keyword
    /// when it is a reference. A reference that does not resolve is left out.
    fn in_place(&mut self, s: &SchemaAt) -> Vec<InPlace> {
        let mut out = Vec::new();
        let l = self.l;
        let Some(m) = l
            .docs
            .get(&s.0)
            .and_then(|d| lookup(d, &s.1).ok())
            .and_then(Value::as_object)
        else {
            return out;
        };
        if self.load(&s.0).is_err() {
            return out;
        }
        let (_, base) = self.resource(&s.0, &s.1);
        for kw in ["$ref", "$dynamicRef", "$recursiveRef"] {
            if let Some(r) = written(m, kw)
                && let Ok(t) = self.resolve(&s.0, &base, r)
                && l.docs.get(&t.0).is_some_and(|d| lookup(d, &t.1).is_ok())
            {
                out.push((format!("/{kw}"), t, Some(kw)));
            }
        }
        if m.contains_key("$ref") && draft_before_2019(&l.docs[&s.0]) {
            return out;
        }
        each_subschema(m, |toks, _| {
            let in_place = matches!(
                toks[0].as_str(),
                "allOf"
                    | "anyOf"
                    | "oneOf"
                    | "not"
                    | "if"
                    | "then"
                    | "else"
                    | "dependentSchemas"
                    | "dependencies"
            );
            if in_place {
                let rel = pointer(toks);
                out.push((encode(&rel), (s.0.clone(), format!("{}{rel}", s.1)), None));
            }
        });
        out
    }

    /// fragment resolves a decoded fragment in the resource at res of the
    /// document u: a JSON pointer, or the name of an "$anchor".
    fn fragment(&self, u: &str, res: &str, frag: &str) -> Result<String, String> {
        if frag.is_empty() || frag.starts_with('/') {
            return Ok(format!("{res}{frag}"));
        }
        let doc = self.l.load(u)?;
        let found = lookup(doc, res)
            .ok()
            .and_then(|v| find_anchor(v, frag, res, true));
        found.ok_or_else(|| {
            let (_, id) = self.resource(u, res);
            format!(
                "anchor in {} not found in schema {}",
                go_quote(&format!("{id}#{}", encode(frag))),
                go_quote(u)
            )
        })
    }
}

/// written is a reference keyword as the schema has it: rewrite keeps the
/// original as "x-docc-go<keyword>" when it changes it.
pub(super) fn written<'a>(m: &'a serde_json::Map<String, Value>, kw: &str) -> Option<&'a str> {
    m.get(&format!("x-docc-go{kw}"))
        .or_else(|| m.get(kw))
        .and_then(Value::as_str)
}

/// find_anchor finds the subschema with "$anchor" (or "$dynamicAnchor")
/// name in v, at the pointer at, without entering another resource.
fn find_anchor(v: &Value, name: &str, at: &str, top: bool) -> Option<String> {
    let m = v.as_object()?;
    if !top && m.contains_key("$id") {
        return None;
    }
    for kw in ["$anchor", "$dynamicAnchor"] {
        if m.get(kw).and_then(Value::as_str) == Some(name) {
            return Some(at.to_string());
        }
    }
    let mut found = None;
    each_subschema(m, |toks, sub| {
        if found.is_none() {
            found = find_anchor(sub, name, &format!("{at}{}", pointer(toks)), false);
        }
    });
    found
}

/// draft_before_2019 reports whether the document declares draft-04, -06 or
/// -07, whose "$ref" hides the keywords next to it.
fn draft_before_2019(doc: &Value) -> bool {
    doc.get("$schema").and_then(Value::as_str).is_some_and(|s| {
        ["draft-04", "draft-06", "draft-07"]
            .iter()
            .any(|d| s.contains(d))
    })
}

/// collect_ids lists the resources of a document: the document itself and
/// every subschema with "$id", with the id resolved against its parent.
fn collect_ids(v: &Value, base: &str, toks: &mut Vec<String>, out: &mut Vec<(String, String)>) {
    let Some(m) = v.as_object() else {
        if toks.is_empty() {
            out.push((String::new(), base.to_string()));
        }
        return;
    };
    let mut base = base.to_string();
    let id = written(m, "$id");
    if let Some(id) = id
        && let Ok((u, _)) = go_join(&base, id)
    {
        base = u;
        out.push((pointer(toks), base.clone()));
    } else if toks.is_empty() {
        out.push((String::new(), base.clone()));
    }
    let mut subs = Vec::new();
    each_subschema(m, |t, sub| subs.push((t.to_vec(), sub)));
    for (t, sub) in subs {
        let n = toks.len();
        toks.extend(t);
        collect_ids(sub, &base, toks, out);
        toks.truncate(n);
    }
}

/// each_subschema calls f with the relative tokens and the value of every
/// subschema of a schema object, "$defs" and "definitions" included.
pub(super) fn each_subschema<'a>(
    m: &'a serde_json::Map<String, Value>,
    mut f: impl FnMut(&[String], &'a Value),
) {
    for (k, v) in m {
        match k.as_str() {
            "not"
            | "if"
            | "then"
            | "else"
            | "contains"
            | "propertyNames"
            | "additionalProperties"
            | "additionalItems"
            | "unevaluatedProperties"
            | "unevaluatedItems"
            | "contentSchema" => f(std::slice::from_ref(k), v),
            "items" if !v.is_array() => f(std::slice::from_ref(k), v),
            "items" | "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                for (i, s) in v.as_array().into_iter().flatten().enumerate() {
                    f(&[k.clone(), i.to_string()], s);
                }
            }
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas"
            | "dependencies" => {
                for (name, s) in v.as_object().into_iter().flatten() {
                    if !s.is_array() {
                        f(&[k.clone(), name.clone()], s);
                    }
                }
            }
            _ => {}
        }
    }
}

/// lookup resolves a JSON pointer in a value the way the Go library does:
/// array items by index. Err(true) is a pointer with a bad ~ escape.
pub(super) fn lookup<'a>(v: &'a Value, ptr: &str) -> Result<&'a Value, bool> {
    let mut v = v;
    if ptr.is_empty() {
        return Ok(v);
    }
    for t in ptr.split('/').skip(1) {
        let t = unescape_token(t).ok_or(true)?;
        let next = match v {
            Value::Object(m) => m.get(&t),
            Value::Array(a) => t
                .parse::<i64>()
                .ok()
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| a.get(i)),
            _ => None,
        };
        v = next.ok_or(false)?;
    }
    Ok(v)
}

/// unescape_token decodes ~0 and ~1; None for any other ~ sequence.
fn unescape_token(t: &str) -> Option<String> {
    let mut out = String::new();
    let mut cs = t.chars();
    while let Some(c) = cs.next() {
        if c != '~' {
            out.push(c);
            continue;
        }
        match cs.next() {
            Some('0') => out.push('~'),
            Some('1') => out.push('/'),
            _ => return None,
        }
    }
    Some(out)
}

/// absolute splits the URL a schema is compiled from into the document URL
/// and the decoded fragment, as the Go compiler does.
fn absolute(input: &str) -> Result<(String, String), String> {
    let (u, f) = input.split_once('#').unwrap_or((input, ""));
    let frag =
        path_unescape(f).map_err(|e| format!("error in parsing {}: {e}", go_quote(input)))?;
    if let Some(e) = go_parse_check(u) {
        return Err(format!(
            "error in parsing {}: parse {}: {e}",
            go_quote(input),
            go_quote(u)
        ));
    }
    Ok((u.to_string(), frag))
}

/// go_join resolves a reference against a base URL as the Go library does
/// with net/url: the resolved URL without fragment, in Go's spelling
/// (`a b.json` becomes `a%20b.json`, `a[b.json` stays), and the decoded
/// fragment.
pub(super) fn go_join(base: &str, r: &str) -> Result<(String, String), String> {
    let parse_err = |s: &str, e: &str| {
        format!(
            "error in parsing {}: parse {}: {e}",
            go_quote(s),
            go_quote(s)
        )
    };
    if let Some(e) = go_parse_check(base) {
        return Err(parse_err(base, &e));
    }
    let (rel, f) = r.split_once('#').unwrap_or((r, ""));
    let frag = path_unescape(f).map_err(|e| format!("error in parsing {}: {e}", go_quote(r)))?;
    if let Some(e) = go_parse_check(rel) {
        return Err(parse_err(rel, &e));
    }
    let b = url::Url::parse(&go_escapes(base)).map_err(|e| parse_err(base, &e.to_string()))?;
    let mut j = if rel.is_empty() {
        b
    } else {
        b.join(&go_escapes(rel))
            .map_err(|e| parse_err(rel, &e.to_string()))?
    };
    j.set_fragment(None);
    Ok((j.to_string(), frag))
}

/// go_escapes percent-encodes the characters of a path that net/url escapes
/// and the url crate leaves alone.
fn go_escapes(s: &str) -> String {
    let (path, query) = s.split_once('?').map_or((s, None), |(p, q)| (p, Some(q)));
    let mut out = path
        .replace('^', "%5E")
        .replace('|', "%7C")
        .replace('\\', "%5C");
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    out
}

/// go_parse_check is the error net/url's Parse gives for s, if any: a
/// control character, or a % not followed by two hex digits before the query.
fn go_parse_check(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.iter().any(|&c| c < 0x20 || c == 0x7f) {
        return Some("net/url: invalid control character in URL".to_string());
    }
    let end = s.find('?').unwrap_or(s.len());
    bad_escape(&b[..end])
}

/// bad_escape is net/url's error for the first % of b that does not start an
/// escape.
fn bad_escape(b: &[u8]) -> Option<String> {
    let hex = |i: usize| b.get(i).is_some_and(u8::is_ascii_hexdigit);
    let i = (0..b.len()).find(|&i| b[i] == b'%' && !(hex(i + 1) && hex(i + 2)))?;
    let bad = String::from_utf8_lossy(&b[i..(i + 3).min(b.len())]).into_owned();
    Some(format!("invalid URL escape {}", go_quote(&bad)))
}

/// path_unescape decodes %XX escapes like net/url's PathUnescape.
fn path_unescape(s: &str) -> Result<String, String> {
    if let Some(e) = bad_escape(s.as_bytes()) {
        return Err(e);
    }
    Ok(super::percent_decode(s))
}

/// encode escapes a JSON pointer for a message the way the Go library does:
/// net/url's PathEscape of every token.
pub(super) fn encode(ptr: &str) -> String {
    ptr.split('/')
        .map(|t| {
            let mut out = String::new();
            for &b in t.as_bytes() {
                if b.is_ascii_alphanumeric() || b"-_.~$&+:=@".contains(&b) {
                    out.push(b as char);
                } else {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// crate_safe makes a URL or reference an RFC 3986 URI for the Rust
/// validator, which rejects what net/url accepts: a space, [ ] { } ^ | " < >
/// ` \, a letter outside ASCII, a % that starts no escape, a second #.
pub(super) fn crate_safe(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    let mut hash = false;
    for (i, &c) in b.iter().enumerate() {
        let keep = c.is_ascii_alphanumeric()
            || b"-._~!$&'()*+,;=:@/?".contains(&c)
            || (c == b'%' && bad_escape(&b[i..(i + 3).min(b.len())]).is_none())
            || (c == b'#' && !std::mem::replace(&mut hash, true));
        if keep {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// rewrite prepares a schema for the Rust validator. "pattern" and the names
/// of "patternProperties" are Go regular expressions: they are rewritten
/// into the regex crate's syntax with Go's meaning, and the original pattern
/// is kept next to a rewritten "pattern" as "x-docc-go-pattern" for the
/// message. References become valid URIs. Returns why the Go library rejects
/// the first bad regular expression, if any.
// ponytail: only subschemas under the keywords of each_subschema are
// rewritten; a "$ref" into an unknown keyword keeps the crate's regex meaning.
pub(super) fn rewrite(v: &mut Value, toks: &mut Vec<String>, bad: &mut Option<String>) {
    let Value::Object(m) = v else {
        return;
    };
    if let Some(Value::String(p)) = m.get("pattern") {
        let p = p.clone();
        match go_pattern(&p) {
            Ok(t) if t != p => {
                m.insert("pattern".to_string(), Value::String(t));
                m.insert("x-docc-go-pattern".to_string(), Value::String(p));
            }
            Ok(_) => {}
            Err(e) => {
                toks.push("pattern".to_string());
                bad.get_or_insert_with(|| {
                    format!(
                        "at {}: {} is not valid regex: {e}",
                        quote(&pointer(toks)),
                        quote(&p)
                    )
                });
                toks.pop();
            }
        }
    }
    if let Some(Value::Object(pp)) = m.get_mut("patternProperties") {
        let old = std::mem::take(pp);
        for (k, s) in old {
            match go_pattern(&k) {
                Ok(t) => {
                    pp.insert(t, s);
                }
                Err(e) => {
                    toks.push("patternProperties".to_string());
                    bad.get_or_insert_with(|| {
                        format!(
                            "at {}: invalid propertyName {} - at '': {} is not valid regex: {e}",
                            quote(&pointer(toks)),
                            quote(&k),
                            quote(&k)
                        )
                    });
                    toks.pop();
                    pp.insert(k, s);
                }
            }
        }
    }
    for kw in ["$ref", "$dynamicRef", "$recursiveRef", "$id"] {
        if let Some(Value::String(r)) = m.get(kw) {
            let safe = crate_safe(r);
            if safe != *r {
                m.insert(format!("x-docc-go{kw}"), Value::String(r.clone()));
                m.insert(kw.to_string(), Value::String(safe));
            }
        }
    }
    let mut subs: Vec<Vec<String>> = Vec::new();
    each_subschema(m, |t, _| subs.push(t.to_vec()));
    for t in subs {
        let mut sub = &mut *v;
        for tok in &t {
            sub = match sub {
                Value::Object(o) => o.get_mut(tok),
                Value::Array(a) => tok.parse::<usize>().ok().and_then(|i| a.get_mut(i)),
                _ => None,
            }
            .expect("each_subschema gives existing paths");
        }
        let n = toks.len();
        toks.extend(t.iter().cloned());
        rewrite(sub, toks, bad);
        toks.truncate(n);
    }
}

/// go_pattern rewrites a Go regular expression for the jsonschema crate. The
/// crate reads a pattern as ECMA-262 and turns it into the regex crate's
/// syntax: \d \w \s become Unicode-aware classes, a '[' inside a class is
/// escaped and \a is refused. compile_go_regexp already gives the regex
/// crate's syntax with Go's meaning; the classes it nests (\d inside [...],
/// [:alpha:]) are flattened into ranges and \a is written as \x07, so the
/// crate passes the pattern through unchanged. Err is Go's reason the
/// pattern is not valid.
pub(super) fn go_pattern(p: &str) -> Result<String, String> {
    let re = cfg::compile_go_regexp(p)?;
    Ok(flatten(re.as_str()))
}

/// flatten replaces every class nested in a class by its ranges and \a by
/// \x07. A negated nested class (\W, [:^alpha:]) becomes the complement of
/// its ranges; under (?i) of the ranges with their other cases, as Go and
/// the regex crate fold a class before they negate it.
fn flatten(t: &str) -> String {
    let cs: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut in_class = false;
    // (?i) now, and as it was outside each open group
    let (mut fold, mut groups) = (false, Vec::new());
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '\\' && i + 1 < cs.len() {
            if cs[i + 1] == 'a' {
                out.push_str(r"\x07");
            } else {
                out.push(c);
                out.push(cs[i + 1]);
            }
            i += 2;
            continue;
        }
        i += 1;
        if !in_class {
            out.push(c);
            if c == '(' {
                // (?flags) sets them for the rest of the group, (?flags:...) inside it
                let flags: String = if cs.get(i) == Some(&'?') {
                    cs[i + 1..]
                        .iter()
                        .take_while(|c| !matches!(c, ':' | ')'))
                        .collect()
                } else {
                    String::new()
                };
                let whole = cs.get(i + 1 + flags.chars().count()) == Some(&')');
                if !whole {
                    groups.push(fold);
                }
                if cs.get(i) == Some(&'?') {
                    let mut on = true;
                    for f in flags.chars() {
                        match f {
                            '-' => on = false,
                            'i' => fold = on,
                            _ => {}
                        }
                    }
                }
            } else if c == ')' {
                fold = groups.pop().unwrap_or(fold);
            }
            if c == '[' {
                in_class = true;
                if cs.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            continue;
        }
        match c {
            ']' => {
                in_class = false;
                out.push(']');
            }
            '[' => {
                // [:name:], [:^name:], or [0-9] / [^0-9] from \d \D and the like
                let rest: String = cs[i..].iter().collect();
                let (neg, ranges, len) = if let Some(r) = rest.strip_prefix(':') {
                    let end = r.find(":]").unwrap_or(r.len());
                    let name = &r[..end];
                    let (neg, name) = name.strip_prefix('^').map_or((false, name), |n| (true, n));
                    (
                        neg,
                        posix(name),
                        name.chars().count() + usize::from(neg) + 3,
                    )
                } else {
                    let (neg, body) = rest
                        .strip_prefix('^')
                        .map_or((false, &rest[..]), |b| (true, b));
                    let end = body.find(']').unwrap_or(body.len());
                    (
                        neg,
                        set_ranges(&body[..end]),
                        body[..end].chars().count() + usize::from(neg) + 1,
                    )
                };
                i += len;
                let ranges = match (neg, fold) {
                    (false, _) => ranges,
                    (true, false) => complement(&ranges),
                    (true, true) => complement(&fold_ascii(&ranges)),
                };
                for (lo, hi) in ranges {
                    if lo == hi {
                        out.push_str(&format!("\\x{{{lo:x}}}"));
                    } else {
                        out.push_str(&format!("\\x{{{lo:x}}}-\\x{{{hi:x}}}"));
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// posix is the ASCII ranges of a [:name:] class.
fn posix(name: &str) -> Vec<(u32, u32)> {
    let s: &[(u8, u8)] = match name {
        "alnum" => &[(b'0', b'9'), (b'A', b'Z'), (b'a', b'z')],
        "alpha" => &[(b'A', b'Z'), (b'a', b'z')],
        "ascii" => &[(0, 0x7f)],
        "blank" => &[(b'\t', b'\t'), (b' ', b' ')],
        "cntrl" => &[(0, 0x1f), (0x7f, 0x7f)],
        "digit" => &[(b'0', b'9')],
        "graph" => &[(b'!', b'~')],
        "lower" => &[(b'a', b'z')],
        "print" => &[(b' ', b'~')],
        "punct" => &[(b'!', b'/'), (b':', b'@'), (b'[', b'`'), (b'{', b'~')],
        "space" => &[(b'\t', b'\r'), (b' ', b' ')],
        "upper" => &[(b'A', b'Z')],
        "word" => &[(b'0', b'9'), (b'A', b'Z'), (b'_', b'_'), (b'a', b'z')],
        "xdigit" => &[(b'0', b'9'), (b'A', b'F'), (b'a', b'f')],
        _ => &[],
    };
    s.iter()
        .map(|&(a, b)| (u32::from(a), u32::from(b)))
        .collect()
}

/// set_ranges reads the items of a simple class body (`0-9A-Za-z_`,
/// `\t\n\f\r `) as sorted ranges.
fn set_ranges(body: &str) -> Vec<(u32, u32)> {
    let mut items = Vec::new();
    let mut cs = body.chars().peekable();
    while let Some(c) = cs.next() {
        let c = if c == '\\' {
            match cs.next() {
                Some('t') => '\t',
                Some('n') => '\n',
                Some('f') => '\x0c',
                Some('r') => '\r',
                Some('v') => '\x0b',
                Some(o) => o,
                None => break,
            }
        } else {
            c
        };
        items.push(c as u32);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        // a '-' between two items is a range (the bodies here hold no literal '-')
        if i + 2 < items.len() && items[i + 1] == '-' as u32 {
            out.push((items[i], items[i + 2]));
            i += 3;
        } else {
            out.push((items[i], items[i]));
            i += 1;
        }
    }
    out.sort_unstable();
    out
}

/// fold_ascii adds the other cases of the ASCII letters in the ranges: the
/// other ASCII case, and K (Kelvin sign) for k, ſ (long s) for s.
fn fold_ascii(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out = ranges.to_vec();
    for &(lo, hi) in ranges {
        for c in lo..=hi.min(0x7f) {
            let ch = char::from(c as u8);
            if ch.is_ascii_alphabetic() {
                let other = if ch.is_ascii_lowercase() {
                    ch.to_ascii_uppercase()
                } else {
                    ch.to_ascii_lowercase()
                };
                out.push((other as u32, other as u32));
                match ch.to_ascii_lowercase() {
                    'k' => out.push((0x212a, 0x212a)),
                    's' => out.push((0x17f, 0x17f)),
                    _ => {}
                }
            }
        }
    }
    out.sort_unstable();
    out
}

/// complement is every scalar value outside the sorted ranges.
fn complement(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut next = 0u32;
    for &(lo, hi) in ranges {
        if lo > next {
            out.push((next, lo - 1));
        }
        next = next.max(hi + 1);
    }
    if next <= 0x10ffff {
        out.push((next, 0x10ffff));
    }
    // \x{d800}-\x{dfff} are not scalar values: split around them
    let mut fixed = Vec::new();
    for (lo, hi) in out {
        if lo < 0xd800 && hi > 0xdfff {
            fixed.push((lo, 0xd7ff));
            fixed.push((0xe000, hi));
        } else if (0xd800..=0xdfff).contains(&lo) && hi > 0xdfff {
            fixed.push((0xe000, hi));
        } else if lo < 0xd800 && (0xd800..=0xdfff).contains(&hi) {
            fixed.push((lo, 0xd7ff));
        } else if !(0xd800..=0xdfff).contains(&lo) {
            fixed.push((lo, hi));
        }
    }
    fixed
}
