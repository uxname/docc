package config

import (
	"strings"

	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/rules"
)

// Setting is the effective level and parameters of one rule for one file.
type Setting struct {
	Level  diag.Level
	Params Params
}

// Resolved holds effective settings of every rule for one file (or none).
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

// apply layers one RuleSet over the current setting of a rule.
// Order of precision: rule entry, thematic group, nursery.
func (c *Config) apply(cur Setting, r *rules.Rule, customName string, rs *RuleSet, preset diag.Level) Setting {
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
	if r.Nursery() && customName == "" {
		if l, ok := rs.ByGroup["nursery"]; ok {
			cur.Level = l
		}
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

// For returns effective settings for a file path relative to src ("" means
// no file: top-level settings only).
func (c *Config) For(rel string) *Resolved {
	var key strings.Builder
	var matched []int
	for i := range c.Overrides {
		if rel != "" && MatchAny(c.Overrides[i].Include, rel) {
			matched = append(matched, i)
			key.WriteByte(byte('a' + i%26))
			key.WriteByte(byte('A' + i/26))
		}
	}
	if c.cache == nil {
		c.cache = map[string]*Resolved{}
	}
	if r, ok := c.cache[key.String()]; ok {
		return r
	}
	res := &Resolved{m: map[string]Setting{}}
	compute := func(r *rules.Rule, custom string, preset diag.Level) {
		s := Setting{Level: preset}
		s = c.apply(s, r, custom, &c.Rules, preset)
		for _, i := range matched {
			s = c.apply(s, r, custom, &c.Overrides[i].Rules, preset)
		}
		k := r.Code
		if custom != "" {
			k = custom
		}
		res.m[k] = s
	}
	for _, r := range rules.All() {
		compute(r, "", c.preset(r))
	}
	dc := rules.ByCode("DOC800")
	for _, cr := range c.Custom {
		compute(dc, cr.ID, cr.Level)
	}
	c.cache[key.String()] = res
	return res
}
