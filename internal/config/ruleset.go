package config

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/rules"
)

// allowedParams lists the parameters each rule accepts besides "level".
var allowedParams = map[string][]string{
	"DOC301": {"words", "allow", "wordsFile", "allowFile"},
	"DOC352": {"words", "allow", "wordsFile", "allowFile"},
	"DOC353": {"words", "allow", "wordsFile", "allowFile"},
	"DOC354": {"words", "allow", "wordsFile", "allowFile"},
	"DOC306": {"allow", "allowFile", "threshold"},
	"DOC313": {"allow", "allowFile"},
	"DOC355": {"allow", "allowFile"},
	"DOC302": {"max"},
	"DOC351": {"max"},
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
	var wordsFile, allowFile []string
	for _, m := range n.Members {
		if m.Key == "level" {
			l, e := w.level(m.Value)
			if e != nil {
				return nil, e
			}
			ent.Level = &l
			continue
		}
		if !contains(allowedParams[rk], m.Key) {
			return nil, w.errAt(m.KeyStart, "parameter %q is not supported by %s", m.Key, rk)
		}
		var e *Error
		switch m.Key {
		case "words":
			ent.Params.Words, e = w.strs(m.Value)
		case "allow":
			ent.Params.Allow, e = w.strs(m.Value)
		case "wordsFile":
			wordsFile, e = w.fileList(m.Value)
		case "allowFile":
			allowFile, e = w.fileList(m.Value)
		case "max":
			ent.Params.Max, e = w.nonNegInt(m.Value)
			ent.Params.HasMax = e == nil
		case "threshold":
			if m.Value.Kind != loader.Number {
				e = w.typeErr(m.Value, "a number")
			} else if _, err := fmt.Sscanf(m.Value.Str, "%g", &ent.Params.Threshold); err != nil || ent.Params.Threshold < 0 || ent.Params.Threshold > 1 {
				e = w.errAt(m.Value.Start, "threshold must be between 0 and 1")
			}
			ent.Params.HasThreshold = e == nil
		}
		if e != nil {
			return nil, e
		}
	}
	for _, f := range wordsFile {
		ws, err := readList(filepath.Join(w.root, filepath.FromSlash(f)))
		if err != nil {
			return nil, &Error{File: w.file, Msg: err.Error()}
		}
		ent.Params.Words = append(ent.Params.Words, ws...)
	}
	for _, f := range allowFile {
		ws, err := readList(filepath.Join(w.root, filepath.FromSlash(f)))
		if err != nil {
			return nil, &Error{File: w.file, Msg: err.Error()}
		}
		ent.Params.Allow = append(ent.Params.Allow, ws...)
	}
	return ent, nil
}

// fileList accepts one path or an array of paths.
func (w *walker) fileList(n *loader.Node) ([]string, *Error) {
	if n.Kind == loader.String {
		return []string{n.Str}, nil
	}
	return w.strs(n)
}

// readList reads a word list: one entry per line, blank lines and lines
// starting with '#' are ignored.
func readList(p string) ([]string, error) {
	data, err := os.ReadFile(p)
	if err != nil {
		return nil, fmt.Errorf("cannot read word list: %v", err)
	}
	text := strings.TrimPrefix(string(data), "\xEF\xBB\xBF")
	var out []string
	for _, ln := range strings.Split(text, "\n") {
		ln = strings.TrimSpace(ln)
		if ln == "" || strings.HasPrefix(ln, "#") {
			continue
		}
		out = append(out, ln)
	}
	return out, nil
}

func (w *walker) overrides(c *Config, n *loader.Node) *Error {
	if n.Kind != loader.Array {
		return w.typeErr(n, "an array")
	}
	for _, it := range n.Items {
		if it.Kind != loader.Object {
			return w.typeErr(it, "an object")
		}
		var ov Override
		haveInclude := false
		ov.Rules = RuleSet{ByRule: map[string]*Entry{}, ByGroup: map[string]diag.Level{}}
		for _, m := range it.Members {
			switch m.Key {
			case "include":
				masks, e := w.strs(m.Value)
				if e != nil {
					return e
				}
				haveInclude = true
				for _, mk := range masks {
					g, err := CompileGlob(mk)
					if err != nil {
						return w.errAt(m.Value.Start, "bad mask %q: %v", mk, err)
					}
					ov.Include = append(ov.Include, g)
				}
			case "rules":
				rs, e := w.ruleSet(c, m.Value)
				if e != nil {
					return e
				}
				ov.Rules = rs
			default:
				return w.errAt(m.KeyStart, "unknown key %q", m.Key)
			}
		}
		if !haveInclude {
			return w.errAt(it.Start, `override needs "include"`)
		}
		c.Overrides = append(c.Overrides, ov)
	}
	return nil
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
					re, err := regexp.Compile(s)
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
