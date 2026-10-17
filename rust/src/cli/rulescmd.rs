//! docc rules and docc explain.

use std::io::Write;

use serde::Serialize;

use super::{Common, Error, load_config, new_flags, no_args, parse, usage_err};
use crate::config::go_quote;
use crate::rules;

#[derive(Debug, Clone, Serialize)]
struct RuleRow {
    code: String,
    rule: String,
    group: String,
    level: String,
    description: String,
}

impl RuleRow {
    fn new(code: &str, rule: &str, group: &str, level: &str, description: &str) -> RuleRow {
        RuleRow {
            code: code.into(),
            rule: rule.into(),
            group: group.into(),
            level: level.into(),
            description: description.into(),
        }
    }
}

/// write_json writes v like Go's json.MarshalIndent(v, "", "  ") and a line
/// break: "<", ">" and "&" are escaped as <, > and &, U+2028
/// and U+2029 as   and  . They can only occur inside strings, so a
/// plain replace is safe.
pub(super) fn write_json(w: &mut dyn Write, v: &impl Serialize) -> Result<(), Error> {
    let s = serde_json::to_string_pretty(v)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")?;
    Ok(())
}

/// write_rows is write_json of a []ruleRow: a nil slice is null.
fn write_rows(w: &mut dyn Write, rows: &[RuleRow]) -> Result<(), Error> {
    if rows.is_empty() {
        w.write_all(b"null\n")?;
        return Ok(());
    }
    write_json(w, &rows)
}

pub(super) fn cmd_rules(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("rules", c), args)?;
    no_args(&pos)?;
    c.check()?;
    let rows = rule_rows(c)?;
    if c.format == "json" {
        write_rows(stdout, &rows)?;
        return Ok(0);
    }
    write_table(stdout, &rows)?;
    Ok(0)
}

/// rule_rows lists the rules with the levels the config gives them; a custom
/// rule stands in the place of the generic DOC800.
fn rule_rows(c: &Common) -> Result<Vec<RuleRow>, Error> {
    let cfg = load_config(c)?;
    let res = cfg.resolve();
    let mut rows = Vec::new();
    for r in rules::all() {
        if r.code == "DOC800" {
            for cr in &cfg.custom {
                rows.push(RuleRow::new("DOC800", &cr.id, "custom", res.custom(&cr.id).level.as_str(), &cr.message));
            }
            continue;
        }
        rows.push(RuleRow::new(r.code, r.name, r.group, res.rule(r.code).level.as_str(), r.desc));
    }
    Ok(rows)
}

/// first_sentence shortens a description for the table.
fn first_sentence(s: &str) -> &str {
    match s.find(". ") {
        Some(i) => &s[..i + 1],
        None => s,
    }
}

fn write_table(w: &mut dyn Write, rows: &[RuleRow]) -> Result<(), Error> {
    let head = RuleRow::new("CODE", "RULE", "GROUP", "LEVEL", "DESCRIPTION");
    let all: Vec<&RuleRow> = std::iter::once(&head).chain(rows).collect();
    let mut wd = [0usize; 4];
    for r in &all {
        for (i, s) in [&r.code, &r.rule, &r.group, &r.level].into_iter().enumerate() {
            wd[i] = wd[i].max(s.chars().count());
        }
    }
    let pad = |s: &str, n: usize| format!("{s}{}", " ".repeat(n - s.chars().count()));
    let mut b = String::new();
    for r in all {
        b.push_str(&format!(
            "{}  {}  {}  {}  {}\n",
            pad(&r.code, wd[0]),
            pad(&r.rule, wd[1]),
            pad(&r.group, wd[2]),
            pad(&r.level, wd[3]),
            first_sentence(&r.description)
        ));
    }
    w.write_all(b.as_bytes())?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct Explanation {
    code: String,
    rule: String,
    group: String,
    level: String,
    description: String,
    bad: String,
    good: String,

    #[serde(skip)]
    core: bool,
    /// the default level, when Level is overridden by the config
    #[serde(skip)]
    def: String,
}

pub(super) fn cmd_explain(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("explain", c), args)?;
    if pos.len() != 1 {
        return Err(usage_err("usage: docc explain <code or rule name>"));
    }
    c.check()?;
    let key = &pos[0];
    if rules::lookup(&key.to_uppercase()).is_none() && rules::is_group(key) {
        // a group: its rules
        let rows: Vec<RuleRow> = rule_rows(c)?.into_iter().filter(|r| &r.group == key).collect();
        if c.format == "json" {
            write_rows(stdout, &rows)?;
            return Ok(0);
        }
        write_table(stdout, &rows)?;
        return Ok(0);
    }
    let e = explain(c, key)?;
    if c.format == "json" {
        write_json(stdout, &e)?;
        return Ok(0);
    }
    let mut level = e.level.clone();
    if !e.def.is_empty() {
        level += &format!(" (default: {})", e.def);
    }
    let text = format!(
        "{} {}\nGroup: {}\nLevel: {level}\n\n{}\n\nViolation:\n  {}\n\nFixed:\n  {}\n\n{}\n",
        e.code,
        e.rule,
        e.group,
        e.description,
        indent(&e.bad),
        indent(&e.good),
        configure(&e)
    );
    stdout.write_all(text.as_bytes())?;
    Ok(0)
}

pub(super) fn indent(s: &str) -> String {
    s.replace('\n', "\n  ")
}

/// configure shows how to change the level of a rule or suppress one finding.
fn configure(e: &Explanation) -> String {
    if e.core {
        return "Configure: this rule cannot be disabled, re-levelled or suppressed.".into();
    }
    let key = if e.code == "DOC800" { &e.rule } else { &e.code };
    format!(
        "Configure (docc.json):\n  \"rules\": {{ \"{key}\": \"off\" }}   (or \"warn\", \"error\")\n\
         Suppress one finding (in the document):\n  \"docIgnore\": [{{ \"rule\": \"{key}\", \"reason\": \"why this is fine\" }}]"
    )
}

fn explain(c: &Common, key: &str) -> Result<Explanation, Error> {
    let cfg = load_config(c);
    if let Some(r) = rules::lookup(&key.to_uppercase()).or_else(|| rules::lookup(key)) {
        let mut e = Explanation {
            code: r.code.into(),
            rule: r.name.into(),
            group: r.group.into(),
            level: r.default.to_string(),
            description: r.desc.into(),
            bad: r.bad.into(),
            good: r.good.into(),
            core: r.core,
            def: String::new(),
        };
        // a broken config must not stop a reference lookup
        if let Ok(cfg) = &cfg
            && !r.core
            && r.code != "DOC800"
        {
            let l = cfg.resolve().rule(r.code).level.to_string();
            if l != e.level {
                e.def = std::mem::replace(&mut e.level, l);
            }
        }
        return Ok(e);
    }
    if rules::is_custom_name(key) {
        let cfg = cfg?;
        for cr in &cfg.custom {
            if cr.id != key {
                continue;
            }
            let (kind, mut ok) = if cr.require.is_some() {
                (
                    format!("a value that does not match {}", cr.pattern),
                    "a value that matches the required expression".to_string(),
                )
            } else {
                (format!("a value that matches {}", cr.pattern), "a value without a match".to_string())
            };
            let desc = format!("{} (custom rule from {}, checks {})", cr.message, cfg.file, cr.given);
            if !cr.hint.is_empty() {
                ok = cr.hint.clone();
            }
            return Ok(Explanation {
                code: "DOC800".into(),
                rule: cr.id.clone(),
                group: "custom".into(),
                level: cfg.resolve().custom(&cr.id).level.to_string(),
                description: desc,
                bad: kind,
                good: ok,
                core: false,
                def: String::new(),
            });
        }
    }
    Err(usage_err(format!(
        "unknown rule {}: use a code like DOC301 or a name like text/forbidden-words",
        go_quote(key)
    )))
}
