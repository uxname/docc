package engine

import (
	"fmt"
	"strings"

	"github.com/santiment/docc/internal/diag"
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

// covers reports whether the entry suppresses the diagnostic.
func (ig *Ignore) covers(dg *diag.Diagnostic) bool {
	if dg.Code == "DOC800" {
		if !ig.AllCust && ig.Custom != dg.Rule {
			return false
		}
	} else if ig.Rule == nil || ig.Rule.Code != dg.Code {
		return false
	}
	if !ig.HasPath {
		return true // the whole document, also diagnostics without a position
	}
	return ig.PathNode != nil && !dg.NoPos && dg.Start >= ig.PathNode.Start && dg.Start < ig.PathNode.End
}

// suppress drops diagnostics covered by docc-ignore and reports entries that
// cover nothing (DOC901).
func (p *Project) suppress() {
	kept := p.Diags[:0]
	for i := range p.Diags {
		dg := &p.Diags[i]
		d := p.byFile[dg.File]
		suppressed := false
		if d != nil && !isCore(dg.Code) {
			for _, ig := range d.Ignores {
				if ig.covers(dg) {
					ig.Used = true
					suppressed = true
				}
			}
		}
		if !suppressed {
			kept = append(kept, *dg)
		}
	}
	p.Diags = kept
	for _, d := range p.Docs {
		for _, ig := range d.Ignores {
			if ig.Used {
				continue
			}
			what := ig.Path
			if ig.PathNode == nil && ig.HasPath {
				p.emit(d, "DOC901", ig.Node.Start, ig.Node.End, fmt.Sprintf("the path %q does not exist in this document", what),
					"Fix the path or remove the docc-ignore entry.")
				continue
			}
			p.emit(d, "DOC901", ig.Node.Start, ig.Node.End, "this docc-ignore entry suppresses nothing",
				"Remove the entry: the rule is off here, or nothing triggers it.")
		}
	}
}

func isCore(code string) bool {
	r := rules.ByCode(code)
	return r != nil && r.Core
}
