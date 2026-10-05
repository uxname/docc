package render

import (
	"regexp"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/loader"
	"golang.org/x/text/collate"
	"golang.org/x/text/language"
)

var reserved = map[string]bool{
	"title": true, "kind": true, "status": true, "parent": true,
	"body": true, "tags": true, "priority": true, "notes": true, "hidden": true,
	"docIgnore": true, "order": true,
}

func (r *Renderer) page(d *engine.Doc) string {
	pg := &page{r: r, doc: d, path: PagePath(d)}
	var blocks []string
	switch {
	case !r.single:
		blocks = append(blocks, Header)
	case r.anchors:
		blocks = append(blocks, pg.anchorTag(""))
	}
	if !r.html { // the HTML file draws title, badges and relations from the data itself
		blocks = append(blocks, "# "+pg.refs(oneLine.Replace(rawTitle(d))))
		if meta := metaLine(d); meta != "" {
			blocks = append(blocks, meta)
		}
		if r.Full && d.Hidden {
			blocks = append(blocks, "> Hidden document.")
		}
	}
	if body := d.Root.Get("body"); body != nil {
		blocks = append(blocks, pg.paragraphs(body, true)...)
	}
	type field struct {
		key, content string
		node         *loader.Node
	}
	var fields []field
	short := 0
	for _, m := range d.Root.Members {
		if !reserved[m.Key] {
			if c := pg.fieldContent(m.Value, 3); c != "" {
				fields = append(fields, field{m.Key, c, m.Value})
				if shortValue(c) {
					short++
				}
			}
		}
	}
	// HTML: one-line values form a property list before the sections, so a page
	// is not a stack of headings with a single word under each
	props := r.html && short >= 2
	if props {
		var items []string
		for _, f := range fields {
			if shortValue(f.content) {
				items = append(items, heading(f.key)+pg.anchorTag(r.anchorOf(d, f.node).anchor)+"\n:   "+f.content)
			}
		}
		blocks = append(blocks, strings.Join(items, "\n\n"))
	}
	for _, f := range fields {
		if props && shortValue(f.content) {
			continue
		}
		if r.anchors {
			blocks = append(blocks, pg.anchorTag(r.anchorOf(d, f.node).anchor))
		}
		blocks = append(blocks, "## "+heading(f.key), f.content)
	}
	if r.Full {
		pg.serviceSections(&blocks)
	}
	if !r.html {
		pg.relations(&blocks)
	}
	return strings.Join(blocks, "\n\n") + "\n"
}

// anchorTag is the target of an in-file link to this page (field == "") or to
// one of its field sections.
// ponytail: ids are <docid>--<anchor>; a document id that itself contains "--" can collide.
func (pg *page) anchorTag(field string) string {
	id := pg.doc.ID
	if field != "" {
		id += "--" + field
	}
	return `<a id="` + id + `"></a>`
}

var blockStart = regexp.MustCompile(`^(- |\d+\. |> |#|\|)`)

// shortValue reports whether a rendered field is one plain line of up to 100
// characters: no list, table, quote or heading.
func shortValue(c string) bool {
	return !strings.Contains(c, "\n") && utf8.RuneCountInString(c) <= 100 && !blockStart.MatchString(c)
}

// fieldContent renders the value of a free field or a section; level is the
// heading level of its subsections.
func (pg *page) fieldContent(v *loader.Node, level int) string {
	if isEmpty(v) {
		return ""
	}
	if engine.IsSections(v) {
		return pg.sections(v, level)
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

// sections renders the members of a set of sections as headings of the given
// level (at most 6), each followed by its content. A section stays visible
// without content: a link to it must have a target.
func (pg *page) sections(v *loader.Node, level int) string {
	var blocks []string
	for _, m := range v.Members {
		if m.Key == engine.SectionsKey {
			continue
		}
		if pg.r.anchors {
			blocks = append(blocks, pg.anchorTag(pg.r.anchorOf(pg.doc, m.Value).anchor))
		}
		blocks = append(blocks, strings.Repeat("#", level)+" "+heading(m.Key))
		if c := pg.fieldContent(m.Value, min(level+1, 6)); c != "" {
			blocks = append(blocks, c)
		}
	}
	return strings.Join(blocks, "\n\n")
}

// serviceSections adds notes and docIgnore to the audit build.
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
}

// entry is one line of the document tree.
type entry struct {
	d     *engine.Doc
	depth int
}

// byTitle sorts siblings by the title shown on the page: case-insensitive,
// numbers by value, Latin before Cyrillic. Equal titles fall back to the id, so
// the order is the same on every build.
func byTitle(c *collate.Collator, list []*engine.Doc) {
	sort.Slice(list, func(i, j int) bool {
		if n := c.CompareString(titleOf(list[i]), titleOf(list[j])); n != 0 {
			return n < 0
		}
		return list[i].ID < list[j].ID
	})
}

// layout nests the documents by parent (siblings by title): first those
// reachable from the roots, then those without a visible parent (with their own children).
func (r *Renderer) layout() (tree, orphans []entry) {
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
	var loose []*engine.Doc
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
			loose = append(loose, d)
		}
	}
	c := collate.New(language.Und, collate.IgnoreCase, collate.Numeric) // a Collator is not safe for concurrent use
	for _, list := range children {
		byTitle(c, list)
	}
	byTitle(c, loose)

	seen := map[string]bool{}
	var out *[]entry
	var emit func(d *engine.Doc, depth int)
	emit = func(d *engine.Doc, depth int) {
		if seen[d.ID] {
			return
		}
		seen[d.ID] = true
		*out = append(*out, entry{d, depth})
		for _, c := range children[d.ID] {
			emit(c, depth+1)
		}
	}
	out = &tree
	for _, d := range roots {
		emit(d, 0)
	}
	out = &orphans
	for _, d := range loose {
		emit(d, 0)
	}
	return tree, orphans
}

// href is the link target of a page: its file, or an in-file anchor in a
// single-file build.
func (r *Renderer) href(d *engine.Doc) string {
	if r.single {
		return "#" + d.ID
	}
	return PagePath(d)
}

// treeLines renders tree entries as a nested list.
func (r *Renderer) treeLines(es []entry) string {
	var lines []string
	for _, e := range es {
		line := strings.Repeat("  ", e.depth) + "- [" + titleOf(e.d) + "](" + r.href(e.d) + ")"
		if r.Full && e.d.Hidden {
			line += " (hidden)"
		}
		lines = append(lines, line)
	}
	return strings.Join(lines, "\n")
}

// treeBlocks are the blocks of the document tree, without the header.
func (r *Renderer) treeBlocks() []string {
	tree, orphans := r.layout()
	var out []string
	if r.Full {
		out = append(out, "> full build: includes hidden documents and notes — do not publish")
	}
	out = append(out, "# Document tree")
	if len(tree) > 0 {
		out = append(out, r.treeLines(tree))
	}
	if len(orphans) > 0 {
		out = append(out, "## Without parent", r.treeLines(orphans))
	}
	if un := r.uncovered(); len(un) > 0 {
		var lines []string
		for _, d := range un {
			lines = append(lines, "- ["+titleOf(d)+"]("+r.href(d)+")")
		}
		out = append(out, "## Documents without code coverage", strings.Join(lines, "\n"))
	}
	return out
}

// uncovered lists the documents of a covered kind no code tag points to
// (DOC403); empty without trace.scan.
func (r *Renderer) uncovered() []*engine.Doc {
	var out []*engine.Doc
	for _, d := range r.docs() {
		if r.P.NeedsCode(d) {
			out = append(out, d)
		}
	}
	return out
}

// tree renders _tree.md: documents nested by parent.
func (r *Renderer) tree() string {
	return strings.Join(append([]string{Header}, r.treeBlocks()...), "\n\n") + "\n"
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

// relations adds the closing block: Parent, Referenced by, Covered by.
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
		if id == d.ID && !r.single {
			*blocks = append(*blocks, "Document tree: [_tree.md](_tree.md)")
			break
		}
	}
}

// backrefs maps a document id to the documents that point at it by parent,
// or [[id]] in the text (notes excluded), ordered by path.
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
		for _, ref := range d.Refs {
			if ref.OK && !ref.Notes {
				add(ref.Target)
			}
		}
	}
	return r.back
}
