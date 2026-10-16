use std::collections::HashMap;

use super::{Config, Entry, Params};
use crate::diag::Level;
use crate::rules::{self, Rule};

/// Setting is the effective level and parameters of one rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Setting {
    pub level: Level,
    pub params: Params,
}

/// Resolved holds effective settings of every rule.
#[derive(Debug, Clone, Default)]
pub struct Resolved {
    /// key: rule code, or custom rule name
    m: HashMap<String, Setting>,
}

impl Resolved {
    /// Rule returns the setting of a built-in rule.
    pub fn rule(&self, code: &str) -> Setting {
        self.m.get(code).cloned().unwrap_or_default()
    }

    /// Custom returns the setting of a custom rule by name.
    pub fn custom(&self, name: &str) -> Setting {
        self.m.get(name).cloned().unwrap_or_default()
    }

    /// On reports whether a built-in rule is enabled.
    pub fn on(&self, code: &str) -> bool {
        self.m.get(code).map_or(Level::default(), |s| s.level) != Level::Off
    }
}

impl Config {
    fn preset(&self, r: &Rule) -> Level {
        if r.core {
            return Level::Error;
        }
        if !self.recommended {
            return Level::Off;
        }
        r.default
    }

    /// apply layers the rules section over the preset setting of a rule.
    /// Order of precision: rule entry, thematic group.
    fn apply(&self, mut cur: Setting, r: &Rule, custom_name: &str, preset: Level) -> Setting {
        let rs = &self.rules;
        if r.core {
            return cur;
        }
        let key = if custom_name.is_empty() {
            r.code
        } else {
            custom_name
        };
        if let Some(e) = rs.by_rule.get(key) {
            return entry_setting(e, preset);
        }
        if !custom_name.is_empty()
            && let Some(Entry { level: Some(l), .. }) = rs.by_rule.get("DOC800")
        {
            cur.level = *l;
            return cur;
        }
        if let Some(l) = rs.by_group.get(r.group) {
            cur.level = *l;
            return cur;
        }
        cur
    }

    /// Resolve returns the effective settings of every rule.
    pub fn resolve(&self) -> &Resolved {
        self.resolved.get_or_init(|| {
            let mut res = Resolved::default();
            for r in rules::all() {
                let p = self.preset(r);
                res.m.insert(
                    r.code.to_string(),
                    self.apply(
                        Setting {
                            level: p,
                            ..Default::default()
                        },
                        r,
                        "",
                        p,
                    ),
                );
            }
            let dc = rules::by_code("DOC800").expect("DOC800 is registered");
            for cr in &self.custom {
                let s = self.apply(
                    Setting {
                        level: cr.level,
                        ..Default::default()
                    },
                    dc,
                    &cr.id,
                    cr.level,
                );
                res.m.insert(cr.id.clone(), s);
            }
            res
        })
    }
}

fn entry_setting(e: &Entry, preset: Level) -> Setting {
    Setting {
        level: e.level.unwrap_or(preset),
        params: e.params.clone(),
    }
}
