package engine

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"

	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/loader"
)

// checkLinks runs DOC206, DOC208 and DOC209.
func (p *Project) checkLinks() {
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) {
			continue
		}
		p.selfLinks(d)
		p.undeclaredLinks(d)
		p.suspectLinks(d)
	}
}

func (p *Project) selfLinks(d *Doc) {
	if n := parentOf(d); n != nil && n.Str == d.ID {
		if dg := p.emit(d, "DOC206", n.Start, n.End, "the document names itself as its parent", "Remove the parent field."); dg != nil {
			for i, m := range d.Root.Members {
				if m.Value == n {
					dg.Fix = &diag.Fix{Edits: []diag.Edit{removeMember(d.Root, i)}}
				}
			}
		}
	}
	if arr := d.Root.Get("links"); arr != nil && arr.Kind == loader.Array {
		for i, it := range arr.Items {
			if it.Kind == loader.String && it.Str == d.ID {
				if dg := p.emit(d, "DOC206", it.Start, it.End, "the document links to itself", "Remove its own id from links."); dg != nil {
					dg.Fix = &diag.Fix{Edits: []diag.Edit{removeItem(arr, i)}}
				}
			}
		}
	}
	for _, r := range d.Refs {
		if r.OK && !r.Notes && r.Target == d.ID {
			p.emit(d, "DOC206", r.Start, r.End, fmt.Sprintf("the text refers to its own document with [[%s]]", r.Inner), "Remove the self-reference.")
		}
	}
}

// declared returns the ids listed in links and parent.
func declared(d *Doc) map[string]bool {
	out := map[string]bool{}
	if n := parentOf(d); n != nil {
		out[n.Str] = true
	}
	for _, n := range StringList(d.Root.Get("links")) {
		out[n.Str] = true
	}
	return out
}

func (p *Project) undeclaredLinks(d *Doc) {
	if p.Off("DOC208") {
		return
	}
	have := declared(d)
	seen := map[string]bool{}
	for _, r := range d.Refs {
		if !r.OK || r.Notes || r.Target == d.ID || have[r.Target] || seen[r.Target] {
			continue
		}
		t := p.Graph[r.Target]
		if t == nil {
			continue // DOC202 speaks
		}
		seen[r.Target] = true
		dg := p.emit(d, "DOC208", r.Start, r.End, fmt.Sprintf("[[%s]] is used in the text but %q is not in links or parent", r.Inner, r.Target),
			fmt.Sprintf("Add %q to links.", r.Target))
		if dg != nil {
			quoted := `"` + r.Target + `"`
			if arr := d.Root.Get("links"); arr != nil && arr.Kind == loader.Array {
				dg.Fix = &diag.Fix{Edits: []diag.Edit{appendItem(arr, quoted)}}
			} else if arr == nil {
				dg.Fix = &diag.Fix{Edits: []diag.Edit{addMember(d.Res.Src, d.Root, "links", "["+quoted+"]")}}
			}
			// links of a wrong type (DOC105): no fix
		}
	}
}

// Fingerprint is the first 8 hex digits of SHA-256 of the canonical JSON of a
// document without reviewed, docc-ignore, $schema and notes.
func Fingerprint(root *loader.Node) string {
	var b strings.Builder
	canonical(&b, root, true)
	sum := sha256.Sum256([]byte(b.String()))
	return hex.EncodeToString(sum[:])[:8]
}

func canonical(b *strings.Builder, n *loader.Node, top bool) {
	switch n.Kind {
	case loader.Null, loader.Bool, loader.Number:
		b.WriteString(n.Str)
	case loader.String:
		quote(b, n.Str)
	case loader.Array:
		b.WriteByte('[')
		for i, it := range n.Items {
			if i > 0 {
				b.WriteByte(',')
			}
			canonical(b, it, false)
		}
		b.WriteByte(']')
	case loader.Object:
		type kv struct {
			k string
			v *loader.Node
		}
		var items []kv
		seen := map[string]bool{}
		for _, m := range n.Members {
			if seen[m.Key] {
				continue
			}
			seen[m.Key] = true
			if top && (m.Key == "reviewed" || m.Key == "docc-ignore" || m.Key == "$schema" || m.Key == "notes") {
				continue
			}
			items = append(items, kv{m.Key, m.Value})
		}
		sort.Slice(items, func(i, j int) bool { return items[i].k < items[j].k })
		b.WriteByte('{')
		for i, it := range items {
			if i > 0 {
				b.WriteByte(',')
			}
			quote(b, it.k)
			b.WriteByte(':')
			canonical(b, it.v, false)
		}
		b.WriteByte('}')
	}
}

// quote writes a JSON string with minimal escaping: quote, backslash, and
// control characters as \u00XX.
func quote(b *strings.Builder, s string) {
	b.WriteByte('"')
	for _, r := range s {
		switch {
		case r == '"':
			b.WriteString(`\"`)
		case r == '\\':
			b.WriteString(`\\`)
		case r < 0x20:
			fmt.Fprintf(b, "\\u%04x", r)
		default:
			b.WriteRune(r)
		}
	}
	b.WriteByte('"')
}

func (p *Project) suspectLinks(d *Doc) {
	if p.Off("DOC209") {
		return
	}
	reviewed := map[string]string{}
	if rn := d.Root.Get("reviewed"); rn != nil && rn.Kind == loader.Object {
		for _, m := range rn.Members {
			if m.Value.Kind == loader.String {
				if _, ok := reviewed[m.Key]; !ok {
					reviewed[m.Key] = m.Value.Str
				}
			}
		}
	}
	type at struct{ start, end int }
	where := map[string]at{}
	var order []string
	add := func(id string, start, end int) {
		if _, ok := where[id]; !ok {
			where[id] = at{start, end}
			order = append(order, id)
		}
	}
	for _, n := range StringList(d.Root.Get("links")) {
		add(n.Str, n.Start, n.End)
	}
	for _, r := range d.Refs {
		if r.OK && r.Notes {
			add(r.Target, r.Start, r.End)
		}
	}
	for _, id := range order {
		t := p.Graph[id]
		if t == nil || !t.Parsed() {
			continue // DOC202 or DOC101 speaks
		}
		want := Fingerprint(t.Root)
		got, ok := reviewed[id]
		w := where[id]
		switch {
		case !ok:
			p.emit(d, "DOC209", w.start, w.end, fmt.Sprintf("the link to %q is not confirmed", id),
				fmt.Sprintf("Check the target, then add %q: %q to reviewed.", id, want))
		case got != want:
			p.emit(d, "DOC209", w.start, w.end, fmt.Sprintf("the target %q changed after the link was reviewed", id),
				fmt.Sprintf("Check the target again, then set reviewed %q to %q.", id, want))
		}
	}
}
