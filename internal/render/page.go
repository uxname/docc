package render

import (
	"sort"
	"strings"

	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/loader"
)

var reserved = map[string]bool{
	"$schema": true, "title": true, "kind": true, "status": true, "parent": true,
	"links": true, "body": true, "tags": true, "notes": true, "hidden": true,
	"reviewed": true, "docc-ignore": true,
}

func (r *Renderer) page(d *engine.Doc) string {
	pg := &page{r: r, doc: d, path: PagePath(d)}
	blocks := []string{Header, "# " + pg.refs(titleOf(d))}
	if r.Full && d.Hidden {
		blocks = append(blocks, "> Скрытый документ.")
	}
	if body := d.Root.Get("body"); body != nil {
		blocks = append(blocks, pg.paragraphs(body, true)...)
	}
	section := func(key string, content string) {
		if content != "" {
			blocks = append(blocks, "## "+heading(key), content)
		}
	}
	for _, k := range []string{"kind", "status"} {
		if v := d.Root.Get(k); v != nil && v.Kind == loader.String {
			section(k, v.Str)
		}
	}
	if v := d.Root.Get("parent"); v != nil && v.Kind == loader.String {
		if t := r.P.Graph[v.Str]; r.hasPage(t) {
			section("parent", pg.link(t, titleOf(t), ""))
		}
	}
	if v := d.Root.Get("links"); v != nil {
		var items []string
		for _, n := range engine.StringList(v) {
			if t := r.P.Graph[n.Str]; r.hasPage(t) {
				items = append(items, "- "+pg.link(t, titleOf(t), ""))
			}
		}
		section("links", strings.Join(items, "\n"))
	}
	if v := d.Root.Get("tags"); v != nil {
		var tags []string
		for _, n := range engine.StringList(v) {
			tags = append(tags, "`"+n.Str+"`")
		}
		section("tags", strings.Join(tags, ", "))
	}
	for _, m := range d.Root.Members {
		if !reserved[m.Key] {
			section(m.Key, pg.fieldContent(m.Value))
		}
	}
	if r.Full {
		pg.serviceSections(&blocks)
	}
	return strings.Join(blocks, "\n\n") + "\n"
}

// fieldContent renders the value of a free field.
func (pg *page) fieldContent(v *loader.Node) string {
	if isEmpty(v) {
		return ""
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

// serviceSections adds notes, docc-ignore and reviewed to the audit build.
func (pg *page) serviceSections(blocks *[]string) {
	add := func(title, content string) {
		if content != "" {
			*blocks = append(*blocks, "## "+title, "> Служебное.", content)
		}
	}
	if n := pg.doc.Root.Get("notes"); n != nil && n.Kind == loader.Array {
		var items []string
		for _, it := range engine.StringList(n) {
			items = append(items, "- "+indentLines(pg.refs(it.Str), "  "))
		}
		add("Заметки (скрытые)", strings.Join(items, "\n"))
	}
	if n := pg.doc.Root.Get("docc-ignore"); n != nil && !isEmpty(n) {
		add("Подавления (docc-ignore)", strings.Join(pg.listLines(n, "", false), "\n"))
	}
	if n := pg.doc.Root.Get("reviewed"); n != nil && !isEmpty(n) {
		add("Подтверждения (reviewed)", strings.Join(pg.listLines(n, "", false), "\n"))
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
			line += " (скрытый)"
		}
		lines = append(lines, line)
		for _, c := range children[d.ID] {
			emit(c, depth+1)
		}
	}
	for _, d := range roots {
		emit(d, 0)
	}
	out := []string{Header, "# Дерево документов"}
	if len(lines) > 0 {
		out = append(out, strings.Join(lines, "\n"))
	}
	lines = nil
	for _, d := range orphans {
		emit(d, 0)
	}
	if len(lines) > 0 {
		out = append(out, "## Без родителя", strings.Join(lines, "\n"))
	}
	return strings.Join(out, "\n\n") + "\n"
}
