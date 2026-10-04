package engine

import (
	"fmt"
	"strings"

	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/rules"
	"github.com/santiment/docc/internal/schema"
)

// Ignore is one valid docc-ignore entry.
type Ignore struct {
	Rule     *rules.Rule // nil for a custom rule named by Custom
	Custom   string      // custom rule name, "" otherwise
	AllCust  bool        // DOC800: every custom rule
	Reason   string
	Path     string
	HasPath  bool
	PathNode *loader.Node // resolved path value, nil when the path does not exist
	Node     *loader.Node // the entry
	Used     bool
}

// matches reports whether the entry applies to the rule (code, plus the
// custom rule name when the diagnostic comes from a custom rule).
func (ig *Ignore) matches(code, custom string) bool {
	if custom != "" {
		return ig.AllCust || ig.Custom == custom
	}
	return ig.Rule != nil && ig.Rule.Code == code
}

// parseIgnores validates docc-ignore of every document (DOC108) and keeps the
// valid entries in Doc.Ignores.
func (p *Project) parseIgnores() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		n := d.Root.Get("docc-ignore")
		if n == nil {
			continue
		}
		bad := func(at *loader.Node, msg, hint string) {
			p.emit(d, "DOC108", at.Start, at.End, msg, hint)
		}
		if n.Kind != loader.Array {
			bad(n, "docc-ignore must be an array of entries", `Write "docc-ignore": [{ "rule": "DOC301", "reason": "why" }].`)
			continue
		}
		for _, it := range n.Items {
			if ig := p.parseIgnore(d, it, bad); ig != nil {
				d.Ignores = append(d.Ignores, ig)
			}
		}
	}
}

func (p *Project) parseIgnore(d *Doc, it *loader.Node, bad func(*loader.Node, string, string)) *Ignore {
	if it.Kind != loader.Object {
		bad(it, "a docc-ignore entry must be an object", `Write { "rule": "DOC301", "reason": "why" }.`)
		return nil
	}
	ig := &Ignore{Node: it}
	var ruleNode, reasonNode *loader.Node
	for _, m := range it.Members {
		switch m.Key {
		case "rule":
			ruleNode = m.Value
		case "reason":
			reasonNode = m.Value
		case "path":
			if m.Value.Kind != loader.String || (m.Value.Str != "" && !strings.HasPrefix(m.Value.Str, "/")) {
				bad(m.Value, "path must be a JSON pointer string", `Write a pointer such as "/statement" or "/body/1".`)
				return nil
			}
			ig.Path, ig.HasPath = m.Value.Str, true
		default:
			p.emit(d, "DOC108", m.KeyStart, m.KeyEnd, fmt.Sprintf("unknown key %q in a docc-ignore entry", m.Key), "Use only rule, reason and path.")
			return nil
		}
	}
	if reasonNode == nil || reasonNode.Kind != loader.String || strings.TrimSpace(reasonNode.Str) == "" {
		bad(it, "a docc-ignore entry needs a reason", `Add "reason": explain why the rule does not apply here.`)
		return nil
	}
	ig.Reason = reasonNode.Str
	if ruleNode == nil || ruleNode.Kind != loader.String {
		bad(it, "a docc-ignore entry needs a rule", `Add "rule": a rule code such as DOC301 or a name such as text/forbidden-words.`)
		return nil
	}
	key := ruleNode.Str
	switch {
	case rules.IsCustomName(key):
		found := false
		for _, c := range p.Cfg.Custom {
			found = found || c.ID == key
		}
		if !found {
			bad(ruleNode, fmt.Sprintf("unknown custom rule %q", key), "Use the id of a rule from customRules, or DOC800 for all of them.")
			return nil
		}
		ig.Custom = key
	case rules.Lookup(key) != nil:
		r := rules.Lookup(key)
		if r.Core {
			bad(ruleNode, fmt.Sprintf("%s is a core rule and cannot be suppressed", r.Code), "Fix the problem instead of suppressing it.")
			return nil
		}
		if r.Code == "DOC800" {
			ig.AllCust = true
		} else {
			ig.Rule = r
		}
	default:
		bad(ruleNode, fmt.Sprintf("unknown rule %q", key), "Use a rule code (DOC301) or name (text/forbidden-words); see docc rules.")
		return nil
	}
	if ig.HasPath {
		ig.PathNode = schema.ResolvePointer(d.Root, ig.Path)
	}
	return ig
}
