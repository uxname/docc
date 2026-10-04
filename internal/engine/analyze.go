package engine

import (
	"strings"

	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/mdscan"
)

// RefUse is one [[id]] or [[id#field]] occurrence in prose.
type RefUse struct {
	Inner      string // text between the brackets
	Target     string // document id (valid only when OK)
	Field      string // anchor field, "" for none
	OK         bool   // the inner text has the form id or id#field
	Start, End int    // source offsets of the whole [[...]]
	Notes      bool
	Ptr        string // pointer of the string that holds it
}

// IncUse is one {{id#field}} occurrence in body or a free field.
type IncUse struct {
	Inner      string
	Target     string
	Field      string
	OK         bool // well formed (id#field)
	Start, End int
	SrcField   string // top-level field that contains the include
	Ptr        string
	Valid      bool // target and field exist and hold text
}

// Scan returns the cached Markdown scan of the string.
func (s *StrNode) Scan() *mdscan.Scan {
	if s.scan == nil {
		s.scan = mdscan.New(s.Node.Str)
	}
	return s.scan
}

// IsText reports whether text rules apply: not in notes.
func (s *StrNode) IsText() bool { return !s.Notes }

// CanInclude reports whether {{...}} is expanded in this string.
func (s *StrNode) CanInclude() bool {
	return !s.Notes && s.Field != "title"
}

func splitRef(inner string) (id, field string, hasField, ok bool) {
	id = inner
	if i := strings.IndexByte(inner, '#'); i >= 0 {
		id, field, hasField = inner[:i], inner[i+1:], true
		if field == "" || strings.Contains(field, "#") {
			return id, field, hasField, false
		}
	}
	return id, field, hasField, ValidID(id)
}

// analyze extracts references and includes from every parsed document.
func (p *Project) analyze() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		strs := d.Strings()
		for i := range strs {
			sn := &strs[i]
			sc := sn.Scan()
			for _, r := range sc.Refs() {
				id, field, _, ok := splitRef(r.Inner)
				a, b := d.Range(sn.Node, r.Start, r.End)
				d.Refs = append(d.Refs, RefUse{Inner: r.Inner, Target: id, Field: field, OK: ok, Start: a, End: b, Notes: sn.Notes, Ptr: sn.Ptr})
			}
			if !sn.CanInclude() {
				continue
			}
			for _, r := range sc.Includes() {
				id, field, hasField, ok := splitRef(r.Inner)
				a, b := d.Range(sn.Node, r.Start, r.End)
				d.Incs = append(d.Incs, IncUse{Inner: r.Inner, Target: id, Field: field, OK: ok && hasField, Start: a, End: b, SrcField: sn.Field, Ptr: sn.Ptr})
			}
		}
	}
}

// StringList returns the string items of an array node (non-strings skipped).
func StringList(n *loader.Node) []*loader.Node {
	if n == nil || n.Kind != loader.Array {
		return nil
	}
	var out []*loader.Node
	for _, it := range n.Items {
		if it.Kind == loader.String {
			out = append(out, it)
		}
	}
	return out
}
