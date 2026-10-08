package config

import (
	"regexp"
	"slices"
	"strings"

	"github.com/uxname/docc/internal/diag"
	"github.com/uxname/docc/internal/loader"
	"github.com/uxname/docc/internal/rules"
	"github.com/uxname/docc/internal/text"
)

// allowedParams lists the parameters each rule accepts besides "level".
var allowedParams = map[string][]string{
	"DOC301": {"words", "allow"},
	"DOC352": {"words", "allow"},
	"DOC353": {"words", "allow"},
	"DOC313": {"allow"},
}

func parseLevel(s string) (diag.Level, bool) {
	switch s {
	case "error":
		return diag.Error, true
	case "warn":
		return diag.Warn, true
	case "off":
		return diag.Off, true
	}
	return 0, false
}

func (w *walker) level(n *loader.Node) (diag.Level, *Error) {
	if n.Kind == loader.String {
		if l, ok := parseLevel(n.Str); ok {
			return l, nil
		}
	}
	return 0, w.errAt(n.Start, `level must be "error", "warn" or "off"`)
}

// resolveKey maps a key of the rules section to a rule identity.
// Returns the entry key (rule code or custom name), the group, or neither.
func (w *walker) resolveKey(c *Config, m loader.Member) (ruleKey, group string, core bool, err *Error) {
	k := m.Key
	if rules.IsGroup(k) {
		return "", k, false, nil
	}
	if rules.IsCustomName(k) {
		for _, cr := range c.Custom {
			if cr.ID == k {
				return k, "", false, nil
			}
		}
		return "", "", false, w.errAt(m.KeyStart, "unknown custom rule %q", k)
	}
	if r := rules.Lookup(k); r != nil {
		return r.Code, "", r.Core, nil
	}
	return "", "", false, w.errAt(m.KeyStart, "unknown rule or group %q", k)
}

func (w *walker) ruleSet(c *Config, n *loader.Node) (RuleSet, *Error) {
	rs := RuleSet{ByRule: map[string]*Entry{}, ByGroup: map[string]diag.Level{}}
	if n.Kind != loader.Object {
		return rs, w.typeErr(n, "an object")
	}
	seenGroup := map[string]bool{}
	for _, m := range n.Members {
		rk, grp, core, e := w.resolveKey(c, m)
		if e != nil {
			return rs, e
		}
		if grp != "" {
			if seenGroup[grp] {
				return rs, w.errAt(m.KeyStart, "duplicate entry for group %q", grp)
			}
			seenGroup[grp] = true
			l, e := w.level(m.Value)
			if e != nil {
				return rs, e
			}
			rs.ByGroup[grp] = l
			continue
		}
		if core {
			return rs, w.errAt(m.KeyStart, "%s is a core rule and cannot be configured", m.Key)
		}
		if _, dup := rs.ByRule[rk]; dup {
			return rs, w.errAt(m.KeyStart, "rule %q is configured twice (by code and by name)", m.Key)
		}
		ent, e := w.entry(rk, m.Value)
		if e != nil {
			return rs, e
		}
		rs.ByRule[rk] = ent
	}
	return rs, nil
}

func (w *walker) entry(rk string, n *loader.Node) (*Entry, *Error) {
	if n.Kind == loader.String {
		l, e := w.level(n)
		if e != nil {
			return nil, e
		}
		return &Entry{Level: &l}, nil
	}
	if n.Kind != loader.Object {
		return nil, w.typeErr(n, `a level or an object with "level" and parameters`)
	}
	ent := &Entry{}
	for _, m := range n.Members {
		if m.Key == "level" {
			l, e := w.level(m.Value)
			if e != nil {
				return nil, e
			}
			ent.Level = &l
			continue
		}
		if !slices.Contains(allowedParams[rk], m.Key) {
			return nil, w.errAt(m.KeyStart, "parameter %q is not supported by %s", m.Key, rk)
		}
		var e *Error
		switch m.Key {
		case "words":
			ent.Params.Words, e = w.strs(m.Value)
			if e == nil {
				for _, x := range ent.Params.Words {
					if !starAtWordEnd(x) {
						e = w.errAt(m.Value.Start, "%q: * is allowed only at the end of a word", x)
					}
				}
			}
		case "allow":
			ent.Params.Allow, e = w.strs(m.Value)
		}
		if e != nil {
			return nil, e
		}
	}
	return ent, nil
}

var customID = regexp.MustCompile(`^custom/[a-z0-9-]+$`)

func (w *walker) customRules(c *Config, n *loader.Node) *Error {
	if n.Kind != loader.Array {
		return w.typeErr(n, "an array")
	}
	seen := map[string]bool{}
	for _, it := range n.Items {
		if it.Kind != loader.Object {
			return w.typeErr(it, "an object")
		}
		cr := &Custom{}
		var haveLevel, haveForbid, haveRequire bool
		for _, m := range it.Members {
			var e *Error
			switch m.Key {
			case "id":
				cr.ID, e = w.str(m.Value)
				if e == nil && !customID.MatchString(cr.ID) {
					e = w.errAt(m.Value.Start, "id must be custom/ followed by a-z, 0-9 and '-'")
				}
				if e == nil && seen[cr.ID] {
					e = w.errAt(m.Value.Start, "duplicate custom rule id %q", cr.ID)
				}
				seen[cr.ID] = true
			case "level":
				cr.Level, e = w.level(m.Value)
				haveLevel = e == nil
			case "message":
				cr.Message, e = w.str(m.Value)
			case "hint":
				cr.Hint, e = w.str(m.Value)
			case "kind":
				cr.Kind, e = w.str(m.Value)
			case "given":
				cr.Given, e = w.str(m.Value)
				if e == nil && !strings.HasPrefix(cr.Given, "/") {
					e = w.errAt(m.Value.Start, "given must be a JSON pointer starting with '/'")
				}
			case "forbid", "require":
				var s string
				s, e = w.str(m.Value)
				if e == nil {
					re, err := regexp.Compile(text.NFC(s)) // the text is matched in NFC too
					if err != nil {
						e = w.errAt(m.Value.Start, "bad regular expression: %v", err)
					} else if m.Key == "forbid" {
						cr.Forbid, haveForbid = re, true
					} else {
						cr.Require, haveRequire = re, true
					}
				}
			default:
				e = w.errAt(m.KeyStart, "unknown key %q", m.Key)
			}
			if e != nil {
				return e
			}
		}
		switch {
		case cr.ID == "":
			return w.errAt(it.Start, `custom rule needs "id"`)
		case !haveLevel || cr.Message == "" || cr.Given == "":
			return w.errAt(it.Start, `custom rule %s needs "level", "message" and "given"`, cr.ID)
		case haveForbid == haveRequire:
			return w.errAt(it.Start, "custom rule %s needs exactly one of forbid and require", cr.ID)
		}
		c.Custom = append(c.Custom, cr)
	}
	return nil
}

// starAtWordEnd checks that * appears only at the end of a space-separated word.
func starAtWordEnd(s string) bool {
	for _, p := range strings.Fields(s) {
		if i := strings.IndexByte(p, '*'); i >= 0 && i != len(p)-1 {
			return false
		}
	}
	return true
}
