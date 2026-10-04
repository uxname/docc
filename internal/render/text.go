package render

import (
	"fmt"
	"strings"

	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/mdscan"
)

// page is the rendering context of one document.
type page struct {
	r    *Renderer
	doc  *engine.Doc
	path string // page path relative to the output directory
}

const maxIncludeDepth = 32

// link renders a link to a document page, with an optional anchor.
func (pg *page) link(target *engine.Doc, text, anchor string) string {
	if pg.r.single {
		href := "#" + target.ID
		if anchor != "" {
			href += "--" + anchor
		}
		return "[" + text + "](" + href + ")"
	}
	href := relPath(pg.path, PagePath(target))
	if anchor != "" {
		href += "#" + anchor
	}
	return "[" + text + "](" + href + ")"
}

// refs replaces [[id]] and [[id#field]] outside code with Markdown links.
func (pg *page) refs(s string) string {
	sc := mdscan.New(s)
	rs := sc.Refs()
	if len(rs) == 0 {
		return s
	}
	var b strings.Builder
	last := 0
	for _, ref := range rs {
		id, field, hasField := strings.Cut(ref.Inner, "#")
		t := pg.r.P.Graph[id]
		if t == nil || !pg.r.hasPage(t) {
			continue // validation rejects this; keep the text
		}
		anc := ""
		if hasField && field != "body" && field != "title" {
			anc = anchor(heading(field))
		}
		b.WriteString(s[last:ref.Start])
		b.WriteString(pg.link(t, titleOf(t), anc))
		last = ref.End
	}
	b.WriteString(s[last:])
	return b.String()
}

// target returns the text elements of an include target field.
func (pg *page) target(id, field string) ([]string, bool) {
	t := pg.r.P.Graph[id]
	if t == nil || !t.Parsed() {
		return nil, false
	}
	v := t.Root.Get(field)
	if v == nil {
		return nil, false
	}
	switch v.Kind {
	case loader.String:
		return []string{v.Str}, true
	case loader.Array:
		var out []string
		for _, it := range v.Items {
			if it.Kind != loader.String {
				return nil, false
			}
			out = append(out, it.Str)
		}
		return out, len(out) > 0
	}
	return nil, false
}

func (pg *page) comment(open bool, inner string) string {
	if open {
		return "<!-- include " + inner + " -->"
	}
	return "<!-- /include -->"
}

// expand resolves {{id#field}} in one element. A whole-element include turns
// into the paragraphs of the target; an include inside a string is replaced by
// the target text joined with spaces.
func (pg *page) expand(s string, depth int) []string {
	if depth > maxIncludeDepth {
		return []string{s}
	}
	sc := mdscan.New(s)
	incs := sc.Includes()
	if len(incs) == 0 {
		return []string{s}
	}
	if len(incs) == 1 && strings.TrimSpace(s) == s[incs[0].Start:incs[0].End] {
		id, field, _ := strings.Cut(incs[0].Inner, "#")
		elems, ok := pg.target(id, field)
		if !ok {
			return []string{s}
		}
		var out []string
		if pg.r.Full {
			out = append(out, pg.comment(true, incs[0].Inner))
		}
		for _, e := range elems {
			out = append(out, pg.expand(e, depth+1)...)
		}
		if pg.r.Full {
			out = append(out, pg.comment(false, ""))
		}
		return out
	}
	var b strings.Builder
	last := 0
	for _, in := range incs {
		id, field, _ := strings.Cut(in.Inner, "#")
		elems, ok := pg.target(id, field)
		if !ok {
			continue
		}
		var parts []string
		for _, e := range elems {
			parts = append(parts, pg.expand(e, depth+1)...)
		}
		b.WriteString(s[last:in.Start])
		if pg.r.Full {
			b.WriteString(pg.comment(true, in.Inner))
		}
		b.WriteString(strings.Join(parts, " "))
		if pg.r.Full {
			b.WriteString(pg.comment(false, ""))
		}
		last = in.End
	}
	b.WriteString(s[last:])
	return []string{b.String()}
}

// paragraphs renders the string values of a field as paragraphs.
func (pg *page) paragraphs(v *loader.Node, includes bool) []string {
	var elems []string
	switch v.Kind {
	case loader.String:
		elems = []string{v.Str}
	case loader.Array:
		for _, it := range v.Items {
			if it.Kind == loader.String {
				elems = append(elems, it.Str)
			}
		}
	}
	var out []string
	for _, e := range elems {
		if includes {
			for _, x := range pg.expand(e, 0) {
				out = append(out, pg.refs(x))
			}
		} else {
			out = append(out, pg.refs(e))
		}
	}
	return out
}

// inline renders a string for use inside a list item: includes joined by
// spaces, refs resolved, further lines indented.
func (pg *page) inline(s string, includes bool) string {
	if includes {
		s = strings.Join(pg.expand(s, 0), " ")
	}
	return pg.refs(s)
}

func indentLines(s, indent string) string {
	return strings.ReplaceAll(s, "\n", "\n"+indent)
}

func scalarText(n *loader.Node) string {
	switch n.Kind {
	case loader.String, loader.Number, loader.Bool:
		return n.Str
	}
	return ""
}

// listLines renders an object or array value as a nested Markdown list.
func (pg *page) listLines(n *loader.Node, indent string, includes bool) []string {
	var out []string
	switch n.Kind {
	case loader.Object:
		for _, m := range n.Members {
			out = append(out, pg.member(m, indent, includes)...)
		}
	case loader.Array:
		allObjects := len(n.Items) > 0
		for _, it := range n.Items {
			allObjects = allObjects && it.Kind == loader.Object
		}
		for i, it := range n.Items {
			marker := "- "
			if allObjects {
				marker = fmt.Sprintf("%d. ", i+1)
			}
			out = append(out, pg.item(it, indent, marker, includes)...)
		}
	}
	return out
}

func isEmpty(n *loader.Node) bool {
	switch n.Kind {
	case loader.Null:
		return true
	case loader.String:
		return n.Str == ""
	case loader.Array:
		return len(n.Items) == 0
	case loader.Object:
		return len(n.Members) == 0
	}
	return false
}

// member renders "- **key:** value" with nested values below.
func (pg *page) member(m loader.Member, indent string, includes bool) []string {
	if isEmpty(m.Value) {
		return nil
	}
	head := indent + "- **" + m.Key + ":**"
	switch m.Value.Kind {
	case loader.Object, loader.Array:
		return append([]string{head}, pg.listLines(m.Value, indent+"  ", includes)...)
	}
	return []string{head + " " + indentLines(pg.inline(scalarText(m.Value), includes && m.Value.Kind == loader.String), indent+"  ")}
}

// item renders one array element with the given list marker.
func (pg *page) item(it *loader.Node, indent, marker string, includes bool) []string {
	if isEmpty(it) {
		return nil
	}
	pad := strings.Repeat(" ", len(marker))
	switch it.Kind {
	case loader.Object:
		lines := pg.listLines(it, indent+pad, includes)
		if len(lines) == 0 {
			return nil
		}
		// the first nested line shares the marker line
		lines[0] = indent + marker + strings.TrimPrefix(lines[0], indent+pad)
		return lines
	case loader.Array:
		lines := pg.listLines(it, indent+pad, includes)
		if len(lines) == 0 {
			return nil
		}
		lines[0] = indent + marker + strings.TrimPrefix(lines[0], indent+pad)
		return lines
	}
	return []string{indent + marker + indentLines(pg.inline(scalarText(it), includes && it.Kind == loader.String), indent+pad)}
}
