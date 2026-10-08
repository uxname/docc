package config

import (
	"github.com/uxname/docc/internal/diag"
	"github.com/uxname/docc/internal/rules"
)

// Setting is the effective level and parameters of one rule.
type Setting struct {
	Level  diag.Level
	Params Params
}

// Resolved holds effective settings of every rule.
type Resolved struct {
	m map[string]Setting // key: rule code, or custom rule name
}

// Rule returns the setting of a built-in rule.
func (r *Resolved) Rule(code string) Setting { return r.m[code] }

// Custom returns the setting of a custom rule by name.
func (r *Resolved) Custom(name string) Setting { return r.m[name] }

// On reports whether a built-in rule is enabled.
func (r *Resolved) On(code string) bool { return r.m[code].Level != diag.Off }

func (c *Config) preset(r *rules.Rule) diag.Level {
	switch {
	case r.Core:
		return diag.Error
	case !c.Recommended:
		return diag.Off
	}
	return r.Default
}

// apply layers the rules section over the preset setting of a rule.
// Order of precision: rule entry, thematic group.
func (c *Config) apply(cur Setting, r *rules.Rule, customName string, preset diag.Level) Setting {
	rs := &c.Rules
	if r.Core {
		return cur
	}
	key := r.Code
	if customName != "" {
		key = customName
	}
	if e, ok := rs.ByRule[key]; ok {
		return entrySetting(e, preset)
	}
	if customName != "" {
		if e, ok := rs.ByRule["DOC800"]; ok && e.Level != nil {
			cur.Level = *e.Level
			return cur
		}
	}
	if l, ok := rs.ByGroup[r.Group]; ok {
		cur.Level = l
		return cur
	}
	return cur
}

func entrySetting(e *Entry, preset diag.Level) Setting {
	s := Setting{Level: preset, Params: e.Params}
	if e.Level != nil {
		s.Level = *e.Level
	}
	return s
}

// Resolve returns the effective settings of every rule.
func (c *Config) Resolve() *Resolved {
	if c.resolved != nil {
		return c.resolved
	}
	res := &Resolved{m: map[string]Setting{}}
	for _, r := range rules.All() {
		res.m[r.Code] = c.apply(Setting{Level: c.preset(r)}, r, "", c.preset(r))
	}
	dc := rules.ByCode("DOC800")
	for _, cr := range c.Custom {
		res.m[cr.ID] = c.apply(Setting{Level: cr.Level}, dc, cr.ID, cr.Level)
	}
	c.resolved = res
	return res
}
