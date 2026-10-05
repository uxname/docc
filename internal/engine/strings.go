package engine

import (
	"strconv"
	"strings"

	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/mdscan"
)

// reservedKeys are top-level keys with a meaning of their own; all other keys
// are free fields that hold prose.
var reservedKeys = map[string]bool{
	"$schema": true, "title": true, "kind": true, "status": true, "parent": true,
	"links": true, "body": true, "tags": true, "notes": true, "hidden": true,
	"reviewed": true, "docIgnore": true,
}

// ServiceFields cannot be anchors or include sources.
var ServiceFields = map[string]bool{
	"notes": true, "docIgnore": true, "reviewed": true, "hidden": true, "$schema": true,
}

// StrNode is one string value of a document with its location.
type StrNode struct {
	Node  *loader.Node
	Ptr   string // JSON pointer of the value
	Field string // top-level key
	Path  string // field plus the section keys down to the value, joined by "/"
	Notes bool   // inside notes: hidden from readers, text rules do not apply
	scan  *mdscan.Scan
}

func escTok(s string) string {
	if !strings.ContainsAny(s, "~/") {
		return s
	}
	return strings.ReplaceAll(strings.ReplaceAll(s, "~", "~0"), "/", "~1")
}

// SectionsKey marks an object whose other keys are section titles.
const SectionsKey = "$sections"

// IsSections reports whether the object is a set of sections: it has the
// member "$sections": true. It counts only where a section may stand (a free
// field or the value of a section); elsewhere it is an ordinary list object.
func IsSections(n *loader.Node) bool {
	m := n.Get(SectionsKey)
	return m != nil && m.Kind == loader.Bool && m.Str == "true"
}

// Resolve finds a free field or a section by path: "field" or "field/section/...".
// A key may itself contain "/", so every split is tried. Only sections are entered.
func Resolve(root *loader.Node, path string) *loader.Node {
	return resolve(root, path, true)
}

func resolve(n *loader.Node, path string, top bool) *loader.Node {
	if n == nil || n.Kind != loader.Object || (!top && !IsSections(n)) {
		return nil
	}
	for _, m := range n.Members {
		if m.Key == SectionsKey {
			continue
		}
		if m.Key == path {
			return m.Value
		}
		if rest, ok := strings.CutPrefix(path, m.Key+"/"); ok && !(top && ServiceFields[m.Key]) {
			if v := resolve(m.Value, rest, false); v != nil {
				return v
			}
		}
	}
	return nil
}

// Strings returns every prose string of the document: title, body, free
// fields at any depth, and notes (marked). Keys of objects are not included.
func (d *Doc) Strings() []StrNode {
	if d.done || d.Root == nil {
		return d.strs
	}
	d.done = true
	for _, m := range d.Root.Members {
		switch {
		case m.Key == "title" || m.Key == "body" || !reservedKeys[m.Key]:
			walk(m.Value, "/"+escTok(m.Key), m.Key, m.Key, !reservedKeys[m.Key], false, &d.strs)
		case m.Key == "notes":
			walk(m.Value, "/notes", m.Key, m.Key, false, true, &d.strs)
		}
	}
	return d.strs
}

// walk collects the strings below n; sec is true where n may be a set of sections.
func walk(n *loader.Node, ptr, field, path string, sec, notes bool, out *[]StrNode) {
	switch n.Kind {
	case loader.String:
		*out = append(*out, StrNode{Node: n, Ptr: ptr, Field: field, Path: path, Notes: notes})
	case loader.Array:
		for i, it := range n.Items {
			walk(it, ptr+"/"+strconv.Itoa(i), field, path, false, notes, out)
		}
	case loader.Object:
		isSec := sec && IsSections(n)
		for _, m := range n.Members {
			p := path
			if isSec {
				p += "/" + m.Key
			}
			walk(m.Value, ptr+"/"+escTok(m.Key), field, p, isSec, notes, out)
		}
	}
}

// Range maps a byte range [a,b) of a decoded string value to source offsets.
func (d *Doc) Range(n *loader.Node, a, b int) (int, int) {
	return d.Res.Src.StrOffset(n, a), d.Res.Src.StrOffset(n, b)
}
