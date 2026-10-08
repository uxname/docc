package engine

import (
	"github.com/uxname/docc/internal/text"
	"regexp"
	"strconv"
	"strings"

	"github.com/uxname/docc/internal/config"
	"github.com/uxname/docc/internal/diag"
	"github.com/uxname/docc/internal/loader"
	"github.com/uxname/docc/internal/mdscan"
)

// pointerTokens splits a JSON pointer; "*" stays as the wildcard token.
func pointerTokens(ptr string) []string {
	var out []string
	for _, t := range strings.Split(ptr[1:], "/") {
		out = append(out, strings.ReplaceAll(strings.ReplaceAll(t, "~1", "/"), "~0", "~"))
	}
	return out
}

// find resolves tokens against a node; "*" matches every array element and
// every object value.
func find(n *loader.Node, tokens []string, out *[]*loader.Node) {
	if len(tokens) == 0 {
		*out = append(*out, n)
		return
	}
	t, rest := tokens[0], tokens[1:]
	switch n.Kind {
	case loader.Object:
		if t == "*" {
			for _, m := range n.Members {
				find(m.Value, rest, out)
			}
		} else if v := n.Get(t); v != nil {
			find(v, rest, out)
		}
	case loader.Array:
		if t == "*" {
			for _, it := range n.Items {
				find(it, rest, out)
			}
		} else if i, err := strconv.Atoi(t); err == nil && strconv.Itoa(i) == t && i >= 0 && i < len(n.Items) {
			find(n.Items[i], rest, out)
		}
	}
}

// textValues returns the strings found by a pointer: a string, or the
// elements of an array of strings. ok is false when a found value is neither.
func textValues(hits []*loader.Node) (strs []*loader.Node, ok bool) {
	for _, h := range hits {
		switch h.Kind {
		case loader.String:
			strs = append(strs, h)
		case loader.Array:
			for _, it := range h.Items {
				if it.Kind != loader.String {
					return nil, false
				}
				strs = append(strs, it)
			}
		default:
			return nil, false
		}
	}
	return strs, true
}

// checkCustom runs the customRules of docc.json (DOC800).
func (p *Project) checkCustom() {
	for _, cr := range p.Cfg.Custom {
		tokens := pointerTokens(cr.Given)
		for _, d := range p.Docs {
			if !d.Parsed() {
				continue
			}
			lvl := p.Cfg.Resolve().Custom(cr.ID).Level
			if lvl == diag.Off {
				continue
			}
			if cr.Kind != "" {
				k := d.Root.Get("kind")
				if k == nil || k.Kind != loader.String || k.Str != cr.Kind {
					continue
				}
			}
			p.runCustom(d, cr, tokens, lvl)
		}
	}
}

func (p *Project) runCustom(d *Doc, cr *config.Custom, tokens []string, lvl diag.Level) {
	var hits []*loader.Node
	find(d.Root, tokens, &hits)
	strs, ok := textValues(hits)
	if !ok || len(strs) == 0 {
		// forbid skips the document; require treats a missing field as a violation
		if cr.Require != nil && len(hits) == 0 {
			p.emitNoPosAt(d, "DOC800", cr.ID, lvl, cr.Message, cr.Hint)
		}
		return
	}
	for _, n := range strs {
		sc := mdscan.New(n.Str)
		if cr.Forbid != nil {
			for _, sp := range sc.Plain {
				seg, back := text.NFCMap(n.Str[sp.Start:sp.End])
				for _, m := range cr.Forbid.FindAllStringIndex(seg, -1) {
					if m[0] == m[1] {
						continue
					}
					s, e := back(m[0], m[1])
					a, b := d.Range(n, sp.Start+s, sp.Start+e)
					p.emitAt(d, "DOC800", cr.ID, lvl, a, b, cr.Message, cr.Hint)
				}
			}
			continue
		}
		if !matchesAny(cr.Require, n.Str, sc) {
			p.emitAt(d, "DOC800", cr.ID, lvl, n.Start, n.End, cr.Message, cr.Hint)
		}
	}
}

func matchesAny(re *regexp.Regexp, s string, sc *mdscan.Scan) bool {
	for _, sp := range sc.Plain {
		if re.MatchString(text.NFC(s[sp.Start:sp.End])) {
			return true
		}
	}
	return false
}

// emitNoPosAt adds a document diagnostic without a position and an explicit
// rule name and level.
func (p *Project) emitNoPosAt(d *Doc, code, name string, lvl diag.Level, msg, hint string) *diag.Diagnostic {
	dg, _ := p.newDiag(d.File, code)
	dg.Rule = name
	dg.DocID = d.ID
	dg.Message, dg.Hint = msg, hint
	dg.Line, dg.Column, dg.EndLine, dg.EndColumn = 1, 1, 1, 1
	dg.NoPos = true
	return p.push(dg, lvl)
}
