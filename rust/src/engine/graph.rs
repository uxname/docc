use std::collections::{HashMap, HashSet};

use super::analyze::IncUse;
use super::project::{Doc, Project};
use super::strings::{
    SECTIONS_KEY, heading_anchor, is_sections, reserved_key, resolve, service_field,
};
use crate::config::go_quote;
use crate::loader::{Kind, Node};
use crate::mdscan;
use crate::schema;
use crate::text;

impl Project {
    /// inGraph reports whether d is the graph node of its id (the first document
    /// by path) and parses.
    pub(super) fn in_graph(&self, d: &Doc) -> bool {
        self.doc(&d.id).is_some_and(|g| std::ptr::eq(g, d))
    }
}

/// parentOf returns the parent value node when it is a string.
pub(super) fn parent_of(d: &Doc) -> Option<&Node> {
    d.root()?.get("parent").filter(|n| n.kind == Kind::String)
}

/// ImageExt lists the image types docc copies into the output.
pub const IMAGE_EXT: [(&str, &str); 6] = [
    (".png", "image/png"),
    (".jpg", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".gif", "image/gif"),
    (".svg", "image/svg+xml"),
    (".webp", "image/webp"),
];

/// image_ext returns the media type of an image extension (Go: ImageExt[ext]).
pub fn image_ext(ext: &str) -> Option<&'static str> {
    IMAGE_EXT.iter().find(|(e, _)| *e == ext).map(|(_, t)| *t)
}

/// path_clean is Go's path.Clean: the shortest '/' path equal to p lexically.
fn path_clean(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let rooted = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for e in p.split('/') {
        match e {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !rooted {
                    out.push("..");
                }
            }
            _ => out.push(e),
        }
    }
    let s = out.join("/");
    match (rooted, s.is_empty()) {
        (true, _) => format!("/{s}"),
        (false, true) => ".".into(),
        _ => s,
    }
}

/// path_dir is Go's path.Dir.
fn path_dir(p: &str) -> String {
    let dir = match p.rfind('/') {
        Some(i) => &p[..i + 1],
        None => "",
    };
    path_clean(dir)
}

/// path_ext is Go's path.Ext.
fn path_ext(p: &str) -> &str {
    for (i, b) in p.bytes().enumerate().rev() {
        match b {
            b'/' => break,
            b'.' => return &p[i..],
            _ => {}
        }
    }
    ""
}

/// ImagePath resolves a relative image path written in the document rel (a
/// path relative to src) to a path relative to src; ok is false when it
/// leaves src.
pub fn image_path(rel: &str, img: &str) -> (String, bool) {
    // path.Join skips empty elements
    let dir = path_dir(rel);
    let joined = match (dir.is_empty(), img.is_empty()) {
        (_, true) => dir,
        (true, false) => img.to_string(),
        _ => format!("{dir}/{img}"),
    };
    let p = path_clean(&joined);
    let ok = p != ".." && !p.starts_with("../");
    (p, ok)
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
struct IncNode {
    doc: String,
    field: String,
}

impl Project {
    /// checkGraph emits DOC201-DOC205 and DOC210-DOC213.
    pub(super) fn check_graph(&self) {
        self.dup_ids();
        self.dup_anchors();
        self.broken_refs();
        self.parent_cycles();
        self.roots();
        self.includes();
        self.broken_files();
    }

    /// brokenFiles emits DOC213 for relative image paths that name no image file in src.
    // ponytail: paths are taken literally; %20 and other URL escapes are not decoded.
    fn broken_files(&self) {
        let src = self.cfg.abs(&self.cfg.src);
        for d in &self.docs {
            if !d.parsed() || !self.in_graph(d) {
                continue;
            }
            for sn in d.strings() {
                for im in sn.scan().images() {
                    if !mdscan::local_image(&im.inner) {
                        continue;
                    }
                    let (a, b) = d.range(sn.node(), im.start, im.end);
                    let (rel, ok) = image_path(&d.rel, &im.inner);
                    let st = std::fs::metadata(src.join(&rel));
                    let msg = if !ok {
                        format!("the image path leaves {}", self.cfg.src)
                    } else if st.as_ref().map_or(true, |m| m.is_dir()) {
                        format!("the image file does not exist: {}/{}", self.cfg.src, rel)
                    } else if image_ext(&text::to_lower(path_ext(&rel))).is_none() {
                        format!(
                            "not an image file docc copies (png, jpg, jpeg, gif, svg, webp): {}",
                            im.inner
                        )
                    } else {
                        String::new()
                    };
                    if !msg.is_empty() {
                        self.emit(
                            d,
                            "DOC213",
                            a,
                            b,
                            msg,
                            format!(
                                "Put the image under {} and write its path relative to the folder of this document.",
                                self.cfg.src
                            ),
                        );
                    }
                }
            }
        }
    }

    fn dup_ids(&self) {
        for d in &self.docs {
            let first = self.doc(&d.id).expect("every document is in the graph");
            if !std::ptr::eq(first, d) {
                self.emit_no_pos(
                    d,
                    "DOC201",
                    format!("id {} is already used by {}", go_quote(&d.id), first.file),
                    "Rename one of the files: the file name (without .json) is the document id and must be unique."
                        .into(),
                );
            }
        }
    }

    fn broken_refs(&self) {
        for d in &self.docs {
            if !d.parsed() || !self.in_graph(d) {
                continue;
            }
            let check = |id: &str, start: usize, end: usize, what: &str, visible_only: bool| {
                let Some(t) = self.doc(id) else {
                    let mut hint = format!(
                        "Fix the id, or create {}/{}.json only if the document is really needed.",
                        self.cfg.src, id
                    );
                    let near = self.similar_ids(id);
                    if !near.is_empty() {
                        hint = format!("Did you mean {}? {hint}", near.join(" or "));
                    }
                    self.emit(
                        d,
                        "DOC202",
                        start,
                        end,
                        format!(
                            "{what} points to a document that does not exist: {}",
                            go_quote(id)
                        ),
                        hint,
                    );
                    return;
                };
                if visible_only && !d.hidden && t.parsed() && t.hidden {
                    self.emit(
                        d,
                        "DOC210",
                        start,
                        end,
                        format!("{what} points to the hidden document {}", go_quote(id)),
                        "A visible document cannot link to a hidden one: make the target visible, or move the reference into notes.".into(),
                    );
                }
            };
            if let Some(n) = parent_of(d) {
                check(&n.str, n.start, n.end, "parent", true);
            }
            for r in &d.refs {
                if !r.ok {
                    self.emit(
                        d,
                        "DOC202",
                        r.start,
                        r.end,
                        format!(
                            "malformed reference {}",
                            go_quote(&format!("[[{}]]", r.inner))
                        ),
                        "Write [[id]] or [[id#field]]: the id uses only a-z, 0-9 and '-'.".into(),
                    );
                    continue;
                }
                check(
                    &r.target,
                    r.start,
                    r.end,
                    &format!("reference [[{}]]", r.inner),
                    !r.notes,
                );
            }
        }
    }

    fn parent_cycles(&self) {
        let mut next: HashMap<&str, &str> = HashMap::new();
        for d in &self.docs {
            if !d.parsed() || !self.in_graph(d) {
                continue;
            }
            if let Some(n) = parent_of(d)
                && self.graph.contains_key(&n.str)
            {
                next.insert(&d.id, &n.str);
            }
        }
        // a node is on a cycle if following parents returns to it
        let mut state: HashMap<&str, u8> = HashMap::new(); // 1 visiting, 2 done
        let mut on_cycle: HashSet<&str> = HashSet::new();
        for d in &self.docs {
            let id = d.id.as_str();
            if !next.contains_key(id) || state.get(id).copied().unwrap_or(0) != 0 {
                continue;
            }
            let mut chain: Vec<&str> = Vec::new();
            let mut cur = id;
            loop {
                match state.get(cur).copied().unwrap_or(0) {
                    1 => {
                        // cycle: from the first occurrence of cur in chain
                        let mut i = chain.len() - 1;
                        while chain[i] != cur {
                            i -= 1;
                        }
                        on_cycle.extend(&chain[i..]);
                        break;
                    }
                    2 => break,
                    _ => {}
                }
                state.insert(cur, 1);
                chain.push(cur);
                let Some(&nx) = next.get(cur) else {
                    break;
                };
                cur = nx;
            }
            for c in chain {
                state.insert(c, 2);
            }
        }
        for d in &self.docs {
            if on_cycle.contains(d.id.as_str()) && self.in_graph(d) {
                let n = parent_of(d).expect("a document on a cycle has a parent");
                self.emit(
                    d,
                    "DOC204",
                    n.start,
                    n.end,
                    format!("the parent chain of {} forms a cycle", go_quote(&d.id)),
                    "Break the cycle: point one document in it to a different parent.".into(),
                );
            }
        }
    }

    /// roots checks DOC205 and DOC203.
    fn roots(&self) {
        let mut starts: Vec<&Doc> = Vec::new();
        for id in &self.cfg.roots {
            let t = self.doc(id);
            if t.is_none_or(|t| t.parsed() && t.hidden) {
                let what = if t.is_some() {
                    "is hidden"
                } else {
                    "does not exist"
                };
                let (mut line, mut col) = (self.cfg.roots_pos[0], self.cfg.roots_pos[1]);
                if line == 0 {
                    (line, col) = (1, 1);
                }
                let hint = format!(
                    "Create {}/{}.json, or change roots in {}.",
                    self.cfg.src, id, self.cfg.file
                );
                let msg = format!("root {} {what}", go_quote(id));
                if self.cfg.roots_pos[0] == 0 {
                    self.emit_file(&self.cfg.file, "DOC205", 1, 1, msg, hint);
                } else {
                    self.emit_file(&self.cfg.file, "DOC205", line, col, msg, hint);
                }
                continue;
            }
            starts.extend(t);
        }
        if starts.is_empty() {
            return; // reachability is undefined without a root
        }
        let visible = |id: &str| {
            // a file with DOC101 stays a node
            self.doc(id).is_some_and(|t| !t.parsed() || !t.hidden)
        };
        let mut links: Vec<(&str, &str)> = Vec::new();
        for d in &self.docs {
            if !d.parsed() || !self.in_graph(d) || d.hidden {
                continue;
            }
            if let Some(n) = parent_of(d)
                && visible(&n.str)
            {
                links.push((&d.id, &n.str));
            }
            for r in &d.refs {
                if r.ok && !r.notes && visible(&r.target) {
                    links.push((&d.id, &r.target));
                }
            }
        }
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for (a, b) in links {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
        for s in &starts {
            seen.insert(&s.id);
            queue.push_back(&s.id);
        }
        while let Some(id) = queue.pop_front() {
            for &nb in adj.get(id).map(Vec::as_slice).unwrap_or_default() {
                if seen.insert(nb) {
                    queue.push_back(nb);
                }
            }
        }
        for d in &self.docs {
            if !d.parsed() || !self.in_graph(d) || d.hidden || seen.contains(d.id.as_str()) {
                continue;
            }
            // a broken parent is already reported (DOC105 / DOC204): no cascade
            if let Some(pn) = d.r().get("parent")
                && (pn.kind != Kind::String || pn.str == d.id)
            {
                continue;
            }
            self.emit_no_pos(
                d,
                "DOC203",
                format!(
                    "document {} is not reachable from the roots [{}]",
                    go_quote(&d.id),
                    self.cfg.roots.join(" ")
                ),
                "Link it from a reachable document ([[id]] in the text) or set its parent.".into(),
            );
        }
    }

    /// dupAnchors emits DOC212: a block anchor repeats in the document or equals
    /// the anchor of a field or section heading of its page.
    // ponytail: numbered heading anchors (scope-1 for a second "scope") are not counted.
    fn dup_anchors(&self) {
        fn sections(n: &Node, heads: &mut HashSet<String>) {
            for m in &n.members {
                if m.key != SECTIONS_KEY {
                    heads.insert(heading_anchor(&m.key));
                    if is_sections(&m.value) {
                        sections(&m.value, heads);
                    }
                }
            }
        }
        for d in &self.docs {
            let Some(root) = d.root() else {
                continue;
            };
            let mut heads = HashSet::new();
            for m in &root.members {
                if !reserved_key(&m.key) {
                    heads.insert(heading_anchor(&m.key));
                    if is_sections(&m.value) {
                        sections(&m.value, &mut heads);
                    }
                }
            }
            for (i, sn) in d.strings().iter().enumerate() {
                if sn.anchor.is_empty() {
                    continue;
                }
                let (a, b) = d.range(sn.node(), sn.anchor_at + 1, sn.node().str.len());
                let name = go_quote(&format!("^{}", sn.anchor));
                if d.blocks.get(&sn.anchor) != Some(&i) {
                    self.emit(
                        d,
                        "DOC212",
                        a,
                        b,
                        format!("block anchor {name} is already used in this document"),
                        "Give each block anchor a name unique within the document.".into(),
                    );
                } else if heads.contains(&sn.anchor) {
                    self.emit(
                        d,
                        "DOC212",
                        a,
                        b,
                        format!("block anchor {name} equals the anchor of a heading on this page"),
                        "Rename the block anchor: a link to it would land on the heading.".into(),
                    );
                }
            }
        }
    }

    /// includes validates {{id#field}} (DOC211) and visibility (DOC210).
    fn includes(&self) {
        let mut edges: HashMap<IncNode, Vec<IncNode>> = HashMap::new();
        for d in &self.docs {
            if !d.parsed() || !self.in_graph(d) {
                continue;
            }
            for u in &d.incs {
                let bad =
                    |msg: String, hint: String| self.emit(d, "DOC211", u.start, u.end, msg, hint);
                if !u.ok {
                    bad(
                        format!("malformed include {}", go_quote(&format!("{{{{{}}}}}", u.inner))),
                        "Write {{id#field}}: the id of an existing document, '#', and a top-level field or field/section with text.".into(),
                    );
                    continue;
                }
                let Some(t) = self.doc(&u.target) else {
                    bad(
                        format!(
                            "include points to a document that does not exist: {}",
                            go_quote(&u.target)
                        ),
                        format!("Create {}/{}.json or fix the id.", self.cfg.src, u.target),
                    );
                    continue;
                };
                let Some(troot) = t.root() else {
                    continue; // DOC101 on the target already
                };
                let to = IncNode {
                    doc: u.target.clone(),
                    field: u.field.clone(),
                };
                if let Some(name) = u.field.strip_prefix('^') {
                    if !t.blocks.contains_key(name) {
                        bad(
                            format!(
                                "document {} has no block anchor {}",
                                go_quote(&u.target),
                                go_quote(&u.field)
                            ),
                            format!(
                                "End a paragraph or list item of the target with \" {}\", or fix the name.",
                                u.field
                            ),
                        );
                        continue;
                    }
                } else if service_field(&u.field) {
                    bad(
                        format!(
                            "include cannot use the service field {}",
                            go_quote(&u.field)
                        ),
                        "Include a content field such as body or a free field.".into(),
                    );
                    continue;
                } else {
                    match resolve(troot, &u.field) {
                        None => {
                            bad(
                                format!(
                                    "document {} has no field {}",
                                    go_quote(&u.target),
                                    go_quote(&u.field)
                                ),
                                "Use an existing top-level field or field/section.".into(),
                            );
                            continue;
                        }
                        Some(v) if !is_text(v) => {
                            bad(
                                format!(
                                    "field {} of {} is not text",
                                    go_quote(&u.field),
                                    go_quote(&u.target)
                                ),
                                "Include a field that is a string or a non-empty array of strings."
                                    .into(),
                            );
                            continue;
                        }
                        _ => {}
                    }
                }
                u.valid.set(true);
                if !d.hidden && t.hidden {
                    self.emit(
                        d,
                        "DOC210",
                        u.start,
                        u.end,
                        format!("include points to the hidden document {}", go_quote(&u.target)),
                        "A visible document cannot include text from a hidden one: make the target visible or remove the include.".into(),
                    );
                }
                let from = IncNode {
                    doc: d.id.clone(),
                    field: u.src_field.clone(),
                };
                edges.entry(from).or_default().push(to.clone());
                if !u.src_anchor.is_empty() {
                    // the anchored paragraph alone is an include source too
                    let from = IncNode {
                        doc: d.id.clone(),
                        field: format!("^{}", u.src_anchor),
                    };
                    edges.entry(from).or_default().push(to);
                }
            }
        }
        let cyclic = cyclic_nodes(&edges);
        if cyclic.is_empty() {
            return;
        }
        // an include is bad when its target reaches a cycle
        let reach = reach_cyclic(&edges, &cyclic);
        for d in &self.docs {
            if !d.parsed() || !self.in_graph(d) {
                continue;
            }
            for u in &d.incs {
                if u.valid.get() && reach.contains(&inc_node(u)) {
                    u.valid.set(false);
                    self.emit(
                        d,
                        "DOC211",
                        u.start,
                        u.end,
                        format!("include {{{{{}}}}} leads into an include cycle", u.inner),
                        "Break the cycle: a field cannot include itself, directly or through other includes.".into(),
                    );
                }
            }
        }
    }

    /// similarIDs returns up to three existing ids close to id, nearest first.
    fn similar_ids(&self, id: &str) -> Vec<String> {
        let mut ids: Vec<&str> = self.graph.keys().map(String::as_str).collect();
        ids.sort_unstable();
        let mut near = Vec::new();
        while near.len() < 3 && !ids.is_empty() {
            let (c, _) = schema::closest(id, &ids);
            if c.is_empty() {
                break;
            }
            ids.retain(|x| *x != c);
            near.push(format!("`{c}`"));
        }
        near
    }
}

fn inc_node(u: &IncUse) -> IncNode {
    IncNode {
        doc: u.target.clone(),
        field: u.field.clone(),
    }
}

fn is_text(n: &Node) -> bool {
    match n.kind {
        Kind::String => true,
        Kind::Array => !n.items.is_empty() && n.items.iter().all(|it| it.kind == Kind::String),
        _ => false,
    }
}

/// cyclicNodes returns nodes that can reach themselves.
fn cyclic_nodes(edges: &HashMap<IncNode, Vec<IncNode>>) -> HashSet<IncNode> {
    let mut out = HashSet::new();
    let mut keys: Vec<&IncNode> = edges.keys().collect();
    keys.sort();
    for k in keys {
        let mut seen: HashSet<&IncNode> = HashSet::new();
        let mut stack: Vec<&IncNode> = edges[k].iter().collect();
        while let Some(n) = stack.pop() {
            if n == k {
                out.insert(k.clone());
                break;
            }
            if !seen.insert(n) {
                continue;
            }
            stack.extend(edges.get(n).into_iter().flatten());
        }
    }
    out
}

/// reachCyclic marks every node that is cyclic or reaches a cyclic node.
fn reach_cyclic(
    edges: &HashMap<IncNode, Vec<IncNode>>,
    cyclic: &HashSet<IncNode>,
) -> HashSet<IncNode> {
    let mut rev: HashMap<&IncNode, Vec<&IncNode>> = HashMap::new();
    for (a, bs) in edges {
        for b in bs {
            rev.entry(b).or_default().push(a);
        }
    }
    let mut out: HashSet<IncNode> = cyclic.clone();
    let mut stack: Vec<&IncNode> = cyclic.iter().collect();
    while let Some(n) = stack.pop() {
        for &pr in rev.get(n).into_iter().flatten() {
            if out.insert(pr.clone()) {
                stack.push(pr);
            }
        }
    }
    out
}
