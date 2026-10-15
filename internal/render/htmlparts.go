package render

import (
	"html"
	"regexp"
	"strings"

	"github.com/uxname/docc/internal/engine"
	"github.com/uxname/docc/internal/loader"
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

// statusRu names the usual statuses in Russian on the badge of the HTML page;
// other values are shown as written.
var statusRu = map[string]string{
	"accepted": "принято", "approved": "согласовано", "done": "готово", "implemented": "реализовано",
	"stable": "действует", "active": "действует", "resolved": "решено", "verified": "проверено",
	"draft": "черновик", "proposed": "предложено", "open": "открыто", "planned": "запланировано",
	"review": "на согласовании", "in-progress": "в работе", "wip": "в работе", "pending": "ожидает",
	"rejected": "отклонено", "deprecated": "устарело", "obsolete": "устарело", "superseded": "заменено",
	"blocked": "заблокировано", "wontfix": "не будет сделано",
}

func (r *Renderer) statusLabel(s string) string {
	if v, ok := statusRu[strings.ToLower(s)]; ok && r.P.Cfg.Lang == "ru" {
		return v
	}
	return s
}

func strField(d *engine.Doc, key string) string {
	if v := d.Root.Get(key); v != nil && (v.Kind == loader.String || v.Kind == loader.Number) {
		return oneLine.Replace(v.Str)
	}
	return ""
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
