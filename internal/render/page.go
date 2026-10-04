package render

import (
	"sort"
	"strings"

	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/loader"
)

var reserved = map[string]bool{
	"$schema": true, "title": true, "kind": true, "status": true, "parent": true,
	"links": true, "body": true, "tags": true, "priority": true, "notes": true, "hidden": true,
	"reviewed": true, "docIgnore": true,
}

func (r *Renderer) page(d *engine.Doc) string {
	pg := &page{r: r, doc: d, path: PagePath(d)}
	blocks := []string{Header, "# " + pg.refs(oneLine.Replace(rawTitle(d)))}
	if meta := metaLine(d); meta != "" {
		blocks = append(blocks, meta)
	}
	if r.Full && d.Hidden {
		blocks = append(blocks, "> Hidden document.")
	}
	if body := d.Root.Get("body"); body != nil {
		blocks = append(blocks, pg.paragraphs(body, true)...)
	}
	for _, m := range d.Root.Members {
		if !reserved[m.Key] {
			if c := pg.fieldContent(m.Value); c != "" {
				blocks = append(blocks, "## "+heading(m.Key), c)
			}
		}
	}
	if r.Full {
		pg.serviceSections(&blocks)
	}
	pg.relations(&blocks)
	return strings.Join(blocks, "\n\n") + "\n"
}

// fieldContent renders the value of a free field.
func (pg *page) fieldContent(v *loader.Node) string {
	if isEmpty(v) {
		return ""
	}
	if t := pg.table(v); t != "" {
		return t
	}
	switch v.Kind {
	case loader.String:
		return strings.Join(pg.paragraphs(v, true), "\n\n")
	case loader.Number, loader.Bool:
		return v.Str
	case loader.Array:
		allStrings := true
		for _, it := range v.Items {
			allStrings = allStrings && it.Kind == loader.String
		}
		if allStrings {
			return strings.Join(pg.paragraphs(v, true), "\n\n")
		}
	}
	return strings.Join(pg.listLines(v, "", true), "\n")
}

// serviceSections adds notes, docIgnore and reviewed to the audit build.
func (pg *page) serviceSections(blocks *[]string) {
	add := func(title, content string) {
		if content != "" {
			*blocks = append(*blocks, "## "+title, "> Service information.", content)
		}
	}
	if n := pg.doc.Root.Get("notes"); n != nil && n.Kind == loader.Array {
		var items []string
		for _, it := range engine.StringList(n) {
			items = append(items, "- "+indentLines(pg.refs(it.Str), "  "))
		}
		add("Notes (hidden)", strings.Join(items, "\n"))
	}
	if n := pg.doc.Root.Get("docIgnore"); n != nil && !isEmpty(n) {
		add("Suppressions (docIgnore)", strings.Join(pg.listLines(n, "", false), "\n"))
	}
	if n := pg.doc.Root.Get("reviewed"); n != nil && !isEmpty(n) {
		add("Reviewed", strings.Join(pg.listLines(n, "", false), "\n"))
	}
}

// tree renders _tree.md: documents nested by parent.
func (r *Renderer) tree() string {
	docs := r.docs()
	isRoot := map[string]bool{}
	var roots []*engine.Doc
	for _, id := range r.P.Cfg.Roots {
		if d := r.P.Graph[id]; r.hasPage(d) && !isRoot[id] {
			isRoot[id] = true
			roots = append(roots, d)
		}
	}
	children := map[string][]*engine.Doc{}
	var orphans []*engine.Doc
	for _, d := range docs {
		if isRoot[d.ID] {
			continue
		}
		pid := ""
		if n := d.Root.Get("parent"); n != nil && n.Kind == loader.String {
			pid = n.Str
		}
		if p := r.P.Graph[pid]; pid != "" && r.hasPage(p) {
			children[pid] = append(children[pid], d)
		} else {
			orphans = append(orphans, d)
		}
	}
	for _, list := range children {
		sort.Slice(list, func(i, j int) bool { return list[i].ID < list[j].ID })
	}
	sort.Slice(orphans, func(i, j int) bool { return orphans[i].ID < orphans[j].ID })

	var lines []string
	seen := map[string]bool{}
	var emit func(d *engine.Doc, depth int)
	emit = func(d *engine.Doc, depth int) {
		if seen[d.ID] {
			return
		}
		seen[d.ID] = true
		line := strings.Repeat("  ", depth) + "- [" + titleOf(d) + "](" + PagePath(d) + ")"
		if r.Full && d.Hidden {
			line += " (hidden)"
		}
		lines = append(lines, line)
		for _, c := range children[d.ID] {
			emit(c, depth+1)
		}
	}
	for _, d := range roots {
		emit(d, 0)
	}
	out := []string{Header}
	if r.Full {
		out = append(out, "> full build: includes hidden documents and notes — do not publish")
	}
	out = append(out, "# Document tree")
	if len(lines) > 0 {
		out = append(out, strings.Join(lines, "\n"))
	}
	lines = nil
	for _, d := range orphans {
		emit(d, 0)
	}
	if len(lines) > 0 {
		out = append(out, "## Without parent", strings.Join(lines, "\n"))
	}
	if len(r.P.Cfg.TraceScan) > 0 {
		lines = nil
		for _, d := range docs {
			if k := d.Root.Get("kind"); k != nil && k.Str == "requirement" && len(r.P.Covers[d.ID]) == 0 {
				lines = append(lines, "- ["+titleOf(d)+"]("+PagePath(d)+")")
			}
		}
		if len(lines) > 0 {
			out = append(out, "## Requirements without code coverage", strings.Join(lines, "\n"))
		}
	}
	return strings.Join(out, "\n\n") + "\n"
}

func rawTitle(d *engine.Doc) string {
	if t := d.Root.Get("title"); t != nil && t.Kind == loader.String {
		return t.Str
	}
	return d.ID
}

// metaLine is the one-line summary under the title: kind · status · priority N · tags.
func metaLine(d *engine.Doc) string {
	var parts []string
	for _, k := range []string{"kind", "status"} {
		if v := d.Root.Get(k); v != nil && v.Kind == loader.String && v.Str != "" {
			parts = append(parts, oneLine.Replace(v.Str))
		}
	}
	if v := d.Root.Get("priority"); v != nil && (v.Kind == loader.Number || v.Kind == loader.String) && v.Str != "" {
		parts = append(parts, "priority "+oneLine.Replace(v.Str))
	}
	var tags []string
	for _, n := range engine.StringList(d.Root.Get("tags")) {
		tags = append(tags, n.Str)
	}
	if len(tags) > 0 {
		parts = append(parts, strings.Join(tags, ", "))
	}
	return strings.Join(parts, " · ")
}

// table renders an array of objects with identical scalar keys as a Markdown
// table; it returns "" for any other shape.
func (pg *page) table(v *loader.Node) string {
	if v.Kind != loader.Array || len(v.Items) == 0 {
		return ""
	}
	var keys []string
	for i, it := range v.Items {
		if it.Kind != loader.Object || len(it.Members) == 0 {
			return ""
		}
		for j, m := range it.Members {
			if m.Value.Kind == loader.Object || m.Value.Kind == loader.Array {
				return ""
			}
			if i == 0 {
				keys = append(keys, m.Key)
			} else if j >= len(keys) || keys[j] != m.Key {
				return ""
			}
		}
		if i > 0 && len(it.Members) != len(keys) {
			return ""
		}
	}
	cell := func(n *loader.Node) string {
		s := strings.ReplaceAll(pg.inline(scalarText(n), n.Kind == loader.String), "|", `\|`)
		return strings.ReplaceAll(lf(s), "\n", "<br>")
	}
	rows := []string{"| " + strings.Join(keys, " | ") + " |", "|" + strings.Repeat(" --- |", len(keys))}
	for _, it := range v.Items {
		var cells []string
		for _, m := range it.Members {
			cells = append(cells, cell(m.Value))
		}
		rows = append(rows, "| "+strings.Join(cells, " | ")+" |")
	}
	return strings.Join(rows, "\n")
}

// relations adds the closing block: Parent, Links, Referenced by, Covered by.
func (pg *page) relations(blocks *[]string) {
	r, d := pg.r, pg.doc
	add := func(title, content string) {
		if content != "" {
			*blocks = append(*blocks, "## "+title, content)
		}
	}
	if v := d.Root.Get("parent"); v != nil && v.Kind == loader.String {
		if t := r.P.Graph[v.Str]; r.hasPage(t) {
			add("Parent", pg.link(t, titleOf(t), ""))
		}
	}
	var items []string
	for _, n := range engine.StringList(d.Root.Get("links")) {
		if t := r.P.Graph[n.Str]; r.hasPage(t) {
			items = append(items, "- "+pg.link(t, titleOf(t), ""))
		}
	}
	add("Links", strings.Join(items, "\n"))
	items = nil
	for _, s := range r.backrefs()[d.ID] {
		items = append(items, "- "+pg.link(s, titleOf(s), ""))
	}
	add("Referenced by", strings.Join(items, "\n"))
	items = nil
	for _, c := range r.P.Covers[d.ID] {
		items = append(items, "- `"+c+"`")
	}
	add("Covered by", strings.Join(items, "\n"))
	for _, id := range r.P.Cfg.Roots {
		if id == d.ID {
			*blocks = append(*blocks, "Document tree: [_tree.md](_tree.md)")
			break
		}
	}
}

// backrefs maps a document id to the documents that point at it by parent,
// links or [[id]] in the text (notes excluded), ordered by path.
func (r *Renderer) backrefs() map[string][]*engine.Doc {
	if r.back != nil {
		return r.back
	}
	r.back = map[string][]*engine.Doc{}
	for _, d := range r.docs() {
		seen := map[string]bool{d.ID: true}
		add := func(id string) {
			if !seen[id] && r.hasPage(r.P.Graph[id]) {
				seen[id] = true
				r.back[id] = append(r.back[id], d)
			}
		}
		if n := d.Root.Get("parent"); n != nil && n.Kind == loader.String {
			add(n.Str)
		}
		for _, n := range engine.StringList(d.Root.Get("links")) {
			add(n.Str)
		}
		for _, ref := range d.Refs {
			if ref.OK && !ref.Notes {
				add(ref.Target)
			}
		}
	}
	return r.back
}
