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
	"reviewed": true, "docc-ignore": true,
}

// ServiceFields cannot be anchors or include sources.
var ServiceFields = map[string]bool{
	"notes": true, "docc-ignore": true, "reviewed": true, "hidden": true, "$schema": true,
}

// StrNode is one string value of a document with its location.
type StrNode struct {
	Node  *loader.Node
	Ptr   string // JSON pointer of the value
	Field string // top-level key
	Notes bool   // inside notes: hidden from readers, text rules do not apply
	scan  *mdscan.Scan
}

func escTok(s string) string {
	if !strings.ContainsAny(s, "~/") {
		return s
	}
	return strings.ReplaceAll(strings.ReplaceAll(s, "~", "~0"), "/", "~1")
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
			walk(m.Value, "/"+escTok(m.Key), m.Key, false, &d.strs)
		case m.Key == "notes":
			walk(m.Value, "/notes", m.Key, true, &d.strs)
		}
	}
	return d.strs
}

func walk(n *loader.Node, ptr, field string, notes bool, out *[]StrNode) {
	switch n.Kind {
	case loader.String:
		*out = append(*out, StrNode{Node: n, Ptr: ptr, Field: field, Notes: notes})
	case loader.Array:
		for i, it := range n.Items {
			walk(it, ptr+"/"+strconv.Itoa(i), field, notes, out)
		}
	case loader.Object:
		for _, m := range n.Members {
			walk(m.Value, ptr+"/"+escTok(m.Key), field, notes, out)
		}
	}
}

// Range maps a byte range [a,b) of a decoded string value to source offsets.
func (d *Doc) Range(n *loader.Node, a, b int) (int, int) {
	return d.Res.Src.StrOffset(n, a), d.Res.Src.StrOffset(n, b)
}
