use std::collections::HashMap;

use super::project::Project;
use crate::comments::{self, KEY, Place};
use crate::config::go_quote;
use crate::loader::Kind;

impl Project {
    /// check_comments reports DOC112, a malformed "$comments", an id used
    /// twice in the project or an answer its form field cannot take, and
    /// DOC215, a comment that lost its place.
    pub(super) fn check_comments(&self) {
        let mut ids: HashMap<String, &str> = HashMap::new();
        for d in &self.docs {
            let Some(v) = d.root().and_then(|r| r.get(KEY)) else {
                continue;
            };
            for p in comments::problems(v) {
                self.emit(d, "DOC112", p.start, p.end, p.msg, p.hint);
            }
            for it in v.items.iter().filter(|_| v.kind == Kind::Array) {
                let Some(id) = it.get("id").filter(|n| n.kind == Kind::String && !n.str.is_empty()) else {
                    continue;
                };
                match ids.get(id.str.as_str()) {
                    Some(first) if *first != d.file => self.emit(
                        d,
                        "DOC112",
                        id.start,
                        id.end,
                        format!("comment id {} is used in {} too", go_quote(&id.str), first),
                        "Give every comment of the project its own id (docc comments apply makes one when the id is left out).".into(),
                    ),
                    Some(_) => {}
                    None => {
                        ids.insert(id.str.clone(), &d.file);
                    }
                }
            }
            let root = d.r();
            let title = |id: &str| self.doc(id).and_then(|t| t.root()?.get("title").map(|n| n.str.clone()));
            let items = v.items.iter().filter(|_| v.kind == Kind::Array);
            for (n, c) in items.filter_map(|it| comments::of_item(it).map(|c| (it, c))) {
                let place = comments::place(root, &c, &title).0;
                // an answer to the field as it reads now must fit it; one to an
                // earlier version of the question (changed) is left as it was given
                if let (Place::Same, Some(a)) = (place, &c.answer)
                    && let Some(m) = crate::schema::resolve_pointer(root, &c.at).and_then(|f| comments::answer_problem(f, a))
                {
                    let at = n.get("answer").unwrap_or(n);
                    self.emit(d, "DOC112", at.start, at.end, format!("comment {}: {m}", go_quote(&c.id)), "Keep the answer the field takes (text and radio: one value; checkbox: the options ticked and at most one own answer), or delete the comment (docc comments rm <id>).".into());
                }
                if place != Place::Lost {
                    continue;
                }
                let at = n.get("at").unwrap_or(n);
                let why = if c.input.is_some() {
                    format!("the form field {} is not at {}", go_quote(c.input.as_deref().unwrap_or_default()), go_quote(&c.at))
                } else if comments::blocks(root).iter().any(|l| l.ptr == c.at) {
                    format!("the words {} are not in the block at {}", go_quote(&c.fragment), go_quote(&c.at))
                } else {
                    format!("no block at {}", go_quote(&c.at))
                };
                self.emit(
                    d,
                    "DOC215",
                    at.start,
                    at.end,
                    format!("comment {} lost its place: {why}", go_quote(&c.id)),
                    "Move it: set \"at\" to the pointer of the block it is about now; or delete it if it is handled (docc comments rm <id>).".into(),
                );
            }
        }
    }
}
