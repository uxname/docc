package engine

import (
	"regexp"
	"strconv"
	"strings"
	"unicode"

	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/mdscan"
)

// reservedKeys are top-level keys with a meaning of their own; all other keys
// are free fields that hold prose.
var reservedKeys = map[string]bool{
	"title": true, "kind": true, "status": true, "parent": true,
	"body": true, "tags": true, "notes": true, "hidden": true,
	"docIgnore": true, "order": true,
}

// ServiceFields cannot be anchors or include sources.
var ServiceFields = map[string]bool{
	"notes": true, "docIgnore": true, "hidden": true,
}

// StrNode is one string value of a document with its location.
type StrNode struct {
	Node  *loader.Node
	Ptr   string // JSON pointer of the value
	Field string // top-level key
	Path  string // field plus the section keys down to the value, joined by "/"
	Notes bool   // inside notes: hidden from readers, text rules do not apply
	// Anchor is the block anchor name of a trailing " ^name", "" for none;
	// the marker starts at AnchorAt.
	Anchor   string
	AnchorAt int
	scan     *mdscan.Scan
}

// blockAnchor is a block anchor marker at the very end of a string. The name
// starts with a letter, so "x ^2" and footnotes [^1] are not anchors.
var blockAnchor = regexp.MustCompile(`\s\^([a-z][a-z0-9-]*)$`)

// AnchorName reports whether s is a valid block anchor name.
func AnchorName(s string) bool { return blockAnchor.MatchString(" ^" + s) }

// Anchor builds a heading anchor: lower case, spaces to '-', punctuation
// dropped, Unicode letters kept.
func Anchor(h string) string {
	var b strings.Builder
	for _, r := range strings.ToLower(h) {
		switch {
		case r == ' ':
			b.WriteByte('-')
		case unicode.IsLetter(r) || unicode.IsDigit(r) || r == '-' || r == '_':
			b.WriteRune(r)
		}
	}
	return b.String()
}

// HeadingAnchor is the anchor of the heading of a field or section key.
func HeadingAnchor(key string) string {
	return Anchor(strings.NewReplacer("_", " ", "-", " ").Replace(key))
}

// SplitAnchor cuts a trailing block anchor marker from s.
func SplitAnchor(s string) (text, name string) {
	if m := blockAnchor.FindStringSubmatchIndex(s); m != nil {
		return s[:m[0]], s[m[2]:m[3]]
	}
	return s, ""
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
			walk(m.Value, "/"+escTok(m.Key), m.Key, m.Key, !reservedKeys[m.Key], false, m.Key != "title", &d.strs)
		case m.Key == "notes":
			walk(m.Value, "/notes", m.Key, m.Key, false, true, false, &d.strs)
		}
	}
	return d.strs
}

// walk collects the strings below n; sec is true where n may be a set of
// sections, blk where a string may carry a block anchor: the value of a field
// or section and an array item, not a member of a list object or table row.
func walk(n *loader.Node, ptr, field, path string, sec, notes, blk bool, out *[]StrNode) {
	switch n.Kind {
	case loader.String:
		sn := StrNode{Node: n, Ptr: ptr, Field: field, Path: path, Notes: notes}
		if blk {
			if m := blockAnchor.FindStringSubmatchIndex(n.Str); m != nil {
				sn.Anchor, sn.AnchorAt = n.Str[m[2]:m[3]], m[0]
			}
		}
		*out = append(*out, sn)
	case loader.Array:
		for i, it := range n.Items {
			walk(it, ptr+"/"+strconv.Itoa(i), field, path, false, notes, blk, out)
		}
	case loader.Object:
		isSec := sec && IsSections(n)
		for _, m := range n.Members {
			p := path
			if isSec {
				p += "/" + m.Key
			}
			walk(m.Value, ptr+"/"+escTok(m.Key), field, p, isSec, notes, blk && isSec, out)
		}
	}
}

// Range maps a byte range [a,b) of a decoded string value to source offsets.
func (d *Doc) Range(n *loader.Node, a, b int) (int, int) {
	return d.Res.Src.StrOffset(n, a), d.Res.Src.StrOffset(n, b)
}
