package render

import (
	"html"
	"regexp"
	"strconv"
	"strings"

	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/loader"
)

var esc = html.EscapeString

// statusClass picks the badge colour for a status value by its common meaning.
func statusClass(s string) string {
	switch strings.ToLower(s) {
	case "accepted", "approved", "done", "implemented", "stable", "active", "resolved", "verified":
		return "ok"
	case "draft", "proposed", "open", "planned", "review", "in-progress", "wip", "pending":
		return "warn"
	case "rejected", "deprecated", "obsolete", "superseded", "blocked", "wontfix":
		return "bad"
	}
	return ""
}

func strField(d *engine.Doc, key string) string {
	if v := d.Root.Get(key); v != nil && (v.Kind == loader.String || v.Kind == loader.Number) {
		return oneLine.Replace(v.Str)
	}
	return ""
}

// badges is the row under the title: kind, status, priority, tags.
func (r *Renderer) badges(d *engine.Doc) string {
	var b strings.Builder
	if k := strField(d, "kind"); k != "" {
		b.WriteString(`<span class="badge kind">` + esc(k) + `</span>`)
	}
	if st := strField(d, "status"); st != "" {
		b.WriteString(`<span class="badge ` + statusClass(st) + `">` + esc(st) + `</span>`)
	}
	if p := strField(d, "priority"); p != "" {
		b.WriteString(`<span class="badge">priority ` + esc(p) + `</span>`)
	}
	if r.Full && d.Hidden {
		b.WriteString(`<span class="badge bad">hidden</span>`)
	}
	for _, t := range engine.StringList(d.Root.Get("tags")) {
		b.WriteString(`<span class="tag">#` + esc(t.Str) + `</span>`)
	}
	if b.Len() == 0 {
		return ""
	}
	return `<div class="badges">` + b.String() + "</div>\n"
}

func (r *Renderer) chip(d *engine.Doc) string {
	return `<a class="chip" href="#` + esc(d.ID) + `">` + esc(r.title(d)) + `</a>`
}

// crumbs is the chain of parents above the title, outermost first.
func (r *Renderer) crumbs(d *engine.Doc) string {
	var chain []*engine.Doc
	seen := map[string]bool{d.ID: true}
	for cur := d; ; {
		n := cur.Root.Get("parent")
		if n == nil || n.Kind != loader.String {
			break
		}
		p := r.P.Graph[n.Str]
		if !r.hasPage(p) || seen[p.ID] {
			break
		}
		seen[p.ID] = true
		chain = append([]*engine.Doc{p}, chain...)
		cur = p
	}
	if len(chain) == 0 {
		return ""
	}
	var parts []string
	for _, p := range chain {
		parts = append(parts, `<a href="#`+esc(p.ID)+`">`+esc(r.title(p))+`</a>`)
	}
	return `<nav class="crumbs" aria-label="Breadcrumb">` + strings.Join(parts, `<span aria-hidden="true">›</span>`) + "</nav>\n"
}

var (
	mdLink    = regexp.MustCompile(`\[([^\]]*)\]\([^)]*\)`)
	htmlNoise = regexp.MustCompile(`<!--.*?-->|<[^>]*>|[*` + "`" + `]`)
)

// summary is the first body paragraph (or free text field) as plain text, cut to about 300 characters.
func (r *Renderer) summary(d *engine.Doc) string {
	pg := &page{r: r, doc: d, path: PagePath(d)}
	body := d.Root.Get("body")
	if body == nil { // no body: the first free text field stands in for it
		for _, m := range d.Root.Members {
			if !reserved[m.Key] && m.Value.Kind == loader.String {
				body = m.Value
				break
			}
		}
	}
	if body == nil {
		return ""
	}
	ps := pg.paragraphs(body, true)
	if len(ps) == 0 {
		return ""
	}
	s := htmlNoise.ReplaceAllString(mdLink.ReplaceAllString(ps[0], "$1"), "")
	s = strings.Join(strings.Fields(s), " ")
	if rs := []rune(s); len(rs) > 300 {
		cut := string(rs[:300])
		if i := strings.LastIndex(cut, " "); i > 200 {
			cut = cut[:i]
		}
		s = strings.TrimSpace(cut) + "…"
	}
	return s
}

// rows lists child pages one per line: title, kind and the start of the body.
func (r *Renderer) rows(kids []*engine.Doc) string {
	if len(kids) == 0 {
		return ""
	}
	var b strings.Builder
	b.WriteString("<h2 class=\"rh\">Pages in this section</h2>\n<div class=\"rows\">\n")
	for _, k := range kids {
		b.WriteString(`<a class="row" href="#` + esc(k.ID) + `"><span class="ct">` + esc(r.title(k)))
		if kind := strField(k, "kind"); kind != "" {
			b.WriteString(` <span class="badge kind">` + esc(kind) + `</span>`)
		}
		b.WriteString("</span>")
		if s := r.summary(k); s != "" {
			b.WriteString(`<span class="cs">` + esc(s) + `</span>`)
		}
		b.WriteString("</a>\n")
	}
	return b.String() + "</div>\n"
}

// related is the closing block of a page: Parent, Referenced by and
// Covered by as compact chips. Documents already shown as parent or as child
// cards are not repeated.
func (r *Renderer) related(d *engine.Doc, kids []*engine.Doc) string {
	shown := map[string]bool{d.ID: true}
	for _, k := range kids {
		shown[k.ID] = true
	}
	var b strings.Builder
	group := func(label, content string) {
		if content != "" {
			b.WriteString(`<div class="rg"><span class="rl">` + label + `</span>` + content + "</div>\n")
		}
	}
	if n := d.Root.Get("parent"); n != nil && n.Kind == loader.String {
		if p := r.P.Graph[n.Str]; r.hasPage(p) {
			shown[p.ID] = true
			group("Parent", r.chip(p))
		}
	}
	var refs, cov []string
	for _, s := range r.backrefs()[d.ID] {
		if !shown[s.ID] {
			refs = append(refs, r.chip(s))
		}
	}
	for _, c := range r.P.Covers[d.ID] {
		cov = append(cov, `<code>`+esc(c)+`</code>`)
	}
	group("Referenced by", strings.Join(refs, ""))
	group("Covered by", strings.Join(cov, ""))
	if b.Len() == 0 {
		return ""
	}
	return "<h2 class=\"rh\">Related</h2>\n<div class=\"related\">\n" + b.String() + "</div>\n"
}

// prevNext links to the neighbours in tree order.
func (r *Renderer) prevNext(docs []*engine.Doc, i int) string {
	var b strings.Builder
	if i > 0 {
		b.WriteString(`<a class="prev" href="#` + esc(docs[i-1].ID) + `"><small>Previous</small><span>` + esc(r.title(docs[i-1])) + `</span></a>`)
	}
	if i+1 < len(docs) {
		b.WriteString(`<a class="next" href="#` + esc(docs[i+1].ID) + `"><small>Next</small><span>` + esc(r.title(docs[i+1])) + `</span></a>`)
	}
	if b.Len() == 0 {
		return ""
	}
	return `<nav class="pn" aria-label="Neighbouring pages">` + b.String() + "</nav>\n"
}

// childrenOf maps a document id to its children, read from tree entries.
func childrenOf(lists ...[]entry) map[string][]*engine.Doc {
	out := map[string][]*engine.Doc{}
	for _, es := range lists {
		var stack []*engine.Doc
		for _, e := range es {
			stack = append(stack[:e.depth:e.depth], e.d)
			if e.depth > 0 {
				p := stack[e.depth-1]
				out[p.ID] = append(out[p.ID], e.d)
			}
		}
	}
	return out
}

// navList renders tree entries as nested lists; a page with children gets a
// toggle button.
func (r *Renderer) navList(es []entry) string {
	var b strings.Builder
	b.WriteString("<ul>")
	prev := 0
	for i, e := range es {
		switch {
		case i == 0:
		case e.depth > prev:
			b.WriteString("<ul>")
		default:
			for k := prev; k > e.depth; k-- {
				b.WriteString("</li></ul>")
			}
			b.WriteString("</li>")
		}
		parent := i+1 < len(es) && es[i+1].depth > e.depth
		if parent {
			b.WriteString(`<li class="parent"><button class="tw" type="button" aria-expanded="true" aria-label="Collapse"></button>`)
		} else {
			b.WriteString("<li>")
		}
		b.WriteString(`<a href="#` + esc(e.d.ID) + `">` + esc(r.title(e.d)))
		if r.Full && e.d.Hidden {
			b.WriteString(` <span class="badge bad">hidden</span>`)
		}
		b.WriteString("</a>")
		prev = e.depth
	}
	for k := prev; k > 0; k-- {
		b.WriteString("</li></ul>")
	}
	return b.String() + "</li></ul>"
}

// navHTML is the sidebar tree: pages by parent, pages without a parent,
// requirements without code coverage.
func (r *Renderer) navHTML() string {
	tree, orphans := r.layout()
	var b strings.Builder
	if len(tree) > 0 {
		b.WriteString("<h2>Pages</h2>\n" + r.navList(tree) + "\n")
	}
	// groups that fold: with many documents they would push the tree off the screen
	group := func(label string, n int, body string) {
		b.WriteString(`<h2 class="gh"><button class="gt" type="button" aria-expanded="true">` + label + ` <span>` + strconv.Itoa(n) + "</span></button></h2>\n<div class=\"gb\">" + body + "</div>\n")
	}
	if len(orphans) > 0 {
		group("Without parent", len(orphans), r.navList(orphans))
	}
	if un := r.uncovered(); len(un) > 0 {
		var l strings.Builder
		l.WriteString("<ul>")
		for _, d := range un {
			l.WriteString(`<li><a href="#` + esc(d.ID) + `">` + esc(r.title(d)) + `</a></li>`)
		}
		l.WriteString("</ul>")
		group("Documents without code coverage", len(un), l.String())
	}
	return b.String()
}
