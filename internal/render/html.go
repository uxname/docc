package render

import (
	"archive/zip"
	"bytes"
	_ "embed"
	"encoding/base64"
	"encoding/json"
	"html"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/uxname/docc/internal/comments"
	"github.com/uxname/docc/internal/engine"
	"github.com/uxname/docc/internal/history"
	"github.com/uxname/docc/internal/loader"
	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/extension"
	"github.com/yuin/goldmark/parser"
	"github.com/yuin/goldmark/renderer"
	"github.com/yuin/goldmark/util"
)

//go:embed page.html
var htmlTemplate string

//go:embed page.css
var htmlCSS string

//go:embed app.js
var htmlJS string

// vendorJS is htm/preact/standalone 3.1.1: Preact with hooks and htm, the
// tagged templates the page script is written in. No build step.
//
//go:embed vendor/htm-preact-standalone.js
var vendorJS string

// HTML returns the single-page application: one self-contained HTML file
// whose script (Preact, embedded) draws the pages from a JSON island. Go
// renders the Markdown of every block to HTML; the page adds the tree,
// search, block history from Git, comments and the Markdown export. Same call
// contract as Files.
func (r *Renderer) HTML() string {
	s := *r
	s.single, s.html = true, true // in-file links; the headings get their ids from the converter
	fnPrefix := ""                // footnote ids are <page id>--fn:1, unique across the pages of the file
	var marker *regexp.Regexp     // the block markers of the page being converted
	md := goldmark.New(
		goldmark.WithExtensions(extension.GFM, extension.DefinitionList, // GFM as on GitHub: tables, autolinks, strikethrough
			extension.NewFootnote(extension.WithFootnoteIDPrefixFunction(func(ast.Node) []byte { return []byte(fnPrefix) }))),
		goldmark.WithParserOptions(parser.WithAutoHeadingID()),
		// no WithUnsafe: raw HTML written by agents is dropped, only <br> and the block markers survive
		goldmark.WithRendererOptions(renderer.WithNodeRenderers(util.Prioritized(brOnly{&marker}, 100))),
	)
	conv := func(d *engine.Doc, src string) string {
		g := &ids{prefix: d.ID, used: map[string]bool{}, nums: map[string]bool{}}
		for _, a := range s.slugs[d] { // filled by s.page
			if a.num != "" {
				g.nums[a.num] = true
			}
		}
		var b bytes.Buffer
		fnPrefix = d.ID + "--"
		ctx := parser.NewContext(parser.WithIDs(g))
		if err := md.Convert([]byte(src), &b, parser.WithContext(ctx)); err != nil {
			return html.EscapeString(src) // goldmark does not fail on a string; keep the text anyway
		}
		return b.String()
	}
	tree, orphans := s.layout()
	kids := childrenOf(tree, orphans)
	data := appData{Lang: s.P.Cfg.Lang, Full: s.Full, Key: "docc:" + filepath.Base(s.P.Cfg.Root)}
	nav := func(es []entry) [][2]any {
		out := [][2]any{}
		for _, e := range es {
			out = append(out, [2]any{e.d.ID, e.depth})
		}
		return out
	}
	data.Nav.Tree, data.Nav.Orphans = nav(tree), nav(orphans)
	for _, d := range s.uncovered() {
		data.Nav.Uncovered = append(data.Nav.Uncovered, d.ID)
	}
	current := map[string]*loader.Node{}
	for _, d := range s.ordered() {
		current[d.Rel] = d.Root
		// a marker name that the text of the page does not contain, so no agent text can cut it
		pg := &page{r: &s, doc: d, path: PagePath(d)}
		src := pg.build()
		for k := 0; k < 8 && strings.Count(src, "<!--"+pg.markTag()+":") != len(pg.marks)-pg.cut; k++ {
			pg = &page{r: &s, doc: d, path: PagePath(d), tag: pg.markTag() + "x"}
			src = pg.build()
		}
		marker = pg.markRe()
		ptrs := history.Pointers(d.Root)
		pd := pageData{ID: d.ID, Title: s.title(d), File: d.Rel, Hidden: s.Full && d.Hidden,
			Kind: strField(d, "kind"), Status: strField(d, "status"), Priority: strField(d, "priority"),
			Summary: s.summary(d), Covers: s.P.Covers[d.ID]}
		pd.StatusClass = statusClass(pd.Status)
		for _, t := range engine.StringList(d.Root.Get("tags")) {
			pd.Tags = append(pd.Tags, t.Str)
		}
		pd.Crumbs = s.crumbIDs(d)
		for _, k := range kids[d.ID] {
			pd.Kids = append(pd.Kids, k.ID)
		}
		if n := d.Root.Get("parent"); n != nil && n.Kind == loader.String && s.hasPage(s.P.Graph[n.Str]) {
			pd.Parent = n.Str
		}
		for _, b := range s.backrefs()[d.ID] {
			pd.Refs = append(pd.Refs, b.ID)
		}
		out := conv(d, src)
		// cut the page at the block markers; footnotes become a block of their own
		locs := marker.FindAllStringSubmatchIndex(out, -1)
		add := func(n *loader.Node, h string) {
			if strings.TrimSpace(h) == "" {
				return
			}
			b := blockData{H: h}
			if n != nil {
				b.P, b.S = ptrs[n], history.Text(n)
			}
			pd.Blocks = append(pd.Blocks, b)
		}
		prev, last := 0, (*loader.Node)(nil)
		for _, l := range locs {
			add(last, out[prev:l[0]])
			k, _ := strconv.Atoi(out[l[2]:l[3]])
			last, prev = pg.marks[k], l[1]
		}
		tail := out[prev:]
		if i := strings.Index(tail, `<div class="footnotes"`); i >= 0 {
			add(last, tail[:i])
			add(nil, tail[i:])
		} else {
			add(last, tail)
		}
		for _, p := range pg.props {
			n := p.node
			if n.Kind == loader.Array && len(n.Items) == 1 {
				n = n.Items[0]
			}
			h := strings.TrimSpace(conv(d, p.md))
			h = strings.TrimSuffix(strings.TrimPrefix(h, "<p>"), "</p>")
			pd.Props = append(pd.Props, propData{K: p.key, ID: p.id, P: ptrs[n], H: h, S: history.Text(n)})
		}
		data.Pages = append(data.Pages, pd)
	}
	if len(data.Pages) > 0 {
		data.Title = data.Pages[0].Title
	}
	data.Hist = history.Load(s.P.Cfg.Abs(s.P.Cfg.Src), current)
	data.Comments, _ = comments.Load(s.P.Cfg.Root) // docc comments reports a broken file
	for i, it := range data.Comments {             // show a comment on its block even after the block moved
		var root *loader.Node
		if d := s.P.Graph[it.Doc]; d != nil {
			root = d.Root
		}
		data.Comments[i].Path, _, _ = comments.Locate(root, it)
	}
	files := r.Files()
	for p, b := range files {
		if strings.HasSuffix(p, ".md") && p != "_tree.md" {
			for i := range data.Pages {
				if PagePath(s.P.Graph[data.Pages[i].ID]) == p {
					data.Pages[i].MD = b
				}
			}
		}
	}
	data.Single = r.Single()
	data.Zip = zipFiles(files)
	brandCSS, brandHead, brandPrint, brandFoot := s.brand()
	data.Brand = brandData{Head: brandHead, Print: brandPrint, Foot: brandFoot}
	title := "docc"
	if data.Title != "" {
		title = data.Title
	}
	out := strings.NewReplacer(
		"{{TITLE}}", esc(title),
		"{{CSS}}", htmlCSS+brandCSS,
		"{{DATA}}", scriptJSON(data),
		"{{VENDOR}}", vendorJS,
		"{{JS}}", s.labels(htmlJS, true),
	).Replace(s.labels(htmlTemplate, false))
	return lf(out)
}

type appData struct {
	Title    string           `json:"title"`
	Lang     string           `json:"lang,omitempty"`
	Full     bool             `json:"full,omitempty"`
	Key      string           `json:"key"` // localStorage key of the comments
	Nav      navData          `json:"nav"`
	Pages    []pageData       `json:"pages"`
	Hist     *history.History `json:"hist"`
	Comments []comments.Item  `json:"comments"`
	Brand    brandData        `json:"brand"`
	Single   string           `json:"single"`
	Zip      string           `json:"zip"`
}

type navData struct {
	Tree      [][2]any `json:"tree"`
	Orphans   [][2]any `json:"orphans"`
	Uncovered []string `json:"uncovered"`
}

type brandData struct {
	Head  string `json:"head,omitempty"`
	Print string `json:"print,omitempty"`
	Foot  string `json:"foot,omitempty"`
}

type pageData struct {
	ID          string      `json:"id"`
	Title       string      `json:"title"`
	File        string      `json:"file"` // path relative to src: the key of the history
	Hidden      bool        `json:"hidden,omitempty"`
	Kind        string      `json:"kind,omitempty"`
	Status      string      `json:"status,omitempty"`
	StatusClass string      `json:"sc,omitempty"`
	Priority    string      `json:"priority,omitempty"`
	Tags        []string    `json:"tags,omitempty"`
	Crumbs      []string    `json:"crumbs,omitempty"`
	Kids        []string    `json:"kids,omitempty"`
	Parent      string      `json:"parent,omitempty"`
	Refs        []string    `json:"refs,omitempty"`
	Covers      []string    `json:"covers,omitempty"`
	Summary     string      `json:"summary,omitempty"`
	Props       []propData  `json:"props,omitempty"`
	Blocks      []blockData `json:"blocks"`
	MD          string      `json:"md"`
}

// blockData is one block of a page: its JSON Pointer and source text ("" for
// headings and footnotes, which have no history) and its HTML.
type blockData struct {
	P string `json:"p,omitempty"`
	S string `json:"s,omitempty"`
	H string `json:"h"`
}

type propData struct {
	K  string `json:"k"`
	ID string `json:"id"`
	P  string `json:"p"`
	S  string `json:"s"`
	H  string `json:"h"`
}

// crumbIDs is the chain of parents above the title, outermost first.
func (r *Renderer) crumbIDs(d *engine.Doc) []string {
	var chain []string
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
		chain = append([]string{p.ID}, chain...)
		cur = p
	}
	return chain
}

// scriptJSON encodes v for a <script type="application/json"> element: every
// "<" is escaped, so no text inside can end the element or open a tag.
func scriptJSON(v any) string {
	var b bytes.Buffer
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	_ = enc.Encode(v) // plain data, cannot fail
	return strings.ReplaceAll(strings.TrimSuffix(b.String(), "\n"), "<", `\u003c`)
}

// zipFiles packs the Markdown folder for the "all pages as .zip" export. The
// file times are fixed, so the build stays deterministic.
func zipFiles(files map[string]string) string {
	var names []string
	for n := range files {
		names = append(names, n)
	}
	sort.Strings(names)
	var b bytes.Buffer
	w := zip.NewWriter(&b)
	for _, n := range names {
		f, err := w.CreateHeader(&zip.FileHeader{Name: "docs/" + n, Method: zip.Deflate, Modified: time.Date(2000, 1, 1, 0, 0, 0, 0, time.UTC)})
		if err == nil {
			_, _ = f.Write([]byte(files[n]))
		}
	}
	_ = w.Close() // writes to memory
	return base64.StdEncoding.EncodeToString(b.Bytes())
}

// ids gives heading ids <prefix>--<anchor>: the page id plus the same anchor
// that [[id#field]] links use, unique within the page. A section number
// (from nums) is left out, so numbering does not move the anchors.
type ids struct {
	prefix string
	used   map[string]bool
	nums   map[string]bool
}

func (g *ids) Generate(value []byte, _ ast.NodeKind) []byte {
	v := string(value)
	if num, rest, ok := strings.Cut(v, " "); ok && g.nums[num] {
		v = rest
	}
	base := g.prefix + "--" + anchor(v)
	id := base
	for n := 1; g.used[id]; n++ {
		id = base + "-" + strconv.Itoa(n)
	}
	g.used[id] = true
	return []byte(id)
}

func (g *ids) Put(value []byte) { g.used[string(value)] = true }

// brOnly renders raw inline HTML as nothing except <br> (table cells use it
// for line breaks) and the anchors <a id="x"> and </a> that mark field targets;
// of the HTML blocks only the block markers of the current page pass.
type brOnly struct{ marker **regexp.Regexp }

var anchorOpen = regexp.MustCompile(`^<a id="[A-Za-z0-9_.-]+">$`)

func (b brOnly) RegisterFuncs(reg renderer.NodeRendererFuncRegisterer) {
	// a block marker of page.mark passes; any other HTML block is dropped as goldmark does
	reg.Register(ast.KindHTMLBlock, func(w util.BufWriter, src []byte, n ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			var raw []byte
			lines := n.Lines()
			for i := 0; i < lines.Len(); i++ {
				seg := lines.At(i)
				raw = append(raw, seg.Value(src)...)
			}
			if hb := n.(*ast.HTMLBlock); hb.HasClosure() {
				raw = append(raw, hb.ClosureLine.Value(src)...)
			}
			if m := (*b.marker).Find(raw); m != nil && len(bytes.TrimSpace(raw)) == len(bytes.TrimSpace(m)) {
				_, _ = w.Write(bytes.TrimSpace(raw))
				_ = w.WriteByte('\n')
			} else {
				_, _ = w.WriteString("<!-- raw HTML omitted -->\n")
			}
		}
		return ast.WalkSkipChildren, nil
	})
	reg.Register(ast.KindRawHTML, func(w util.BufWriter, src []byte, n ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			var tag []byte
			raw := n.(*ast.RawHTML)
			for i := 0; i < raw.Segments.Len(); i++ {
				seg := raw.Segments.At(i)
				tag = append(tag, seg.Value(src)...)
			}
			switch t := strings.ToLower(strings.ReplaceAll(string(tag), " ", "")); {
			case t == "<br>" || t == "<br/>":
				_, _ = w.WriteString("<br>")
			case t == "</a>" || anchorOpen.MatchString(string(tag)):
				_, _ = w.WriteString(string(tag))
			default:
				_, _ = w.WriteString("<!-- raw HTML omitted -->")
			}
		}
		return ast.WalkSkipChildren, nil
	})
}
