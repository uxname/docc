package render

import (
	"bytes"
	_ "embed"
	"html"
	"regexp"
	"strconv"
	"strings"

	"github.com/uxname/docc/internal/engine"
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

//go:embed page.js
var htmlJS string

// HTML returns every page in one self-contained HTML file. With scripts it
// shows one page at a time (document tree, search, table of contents, page
// neighbours); without them every page follows the other, as in print. Same
// call contract as Files.
func (r *Renderer) HTML() string {
	s := *r
	s.single, s.html = true, true // in-file links; the headings get their ids from the converter
	fnPrefix := ""                // footnote ids are <page id>--fn:1, unique across the pages of the file
	md := goldmark.New(
		goldmark.WithExtensions(extension.GFM, extension.DefinitionList, // GFM as on GitHub: tables, autolinks, strikethrough
			extension.NewFootnote(extension.WithFootnoteIDPrefixFunction(func(ast.Node) []byte { return []byte(fnPrefix) }))),
		goldmark.WithParserOptions(parser.WithAutoHeadingID()),
		// no WithUnsafe: raw HTML written by agents is dropped, only <br> survives
		goldmark.WithRendererOptions(renderer.WithNodeRenderers(util.Prioritized(brOnly{}, 100))),
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
	docs := s.ordered()
	title := "docc"
	var content strings.Builder
	for i, d := range docs {
		if i == 0 {
			title = s.title(d)
		}
		content.WriteString(`<section class="page" id="` + esc(d.ID) + `" data-title="` + esc(s.title(d)) + "\">\n")
		content.WriteString(s.crumbs(d) + "<h1>" + esc(s.title(d)) + "</h1>\n" + s.badges(d))
		content.WriteString("<div class=\"doc\">\n" + conv(d, s.page(d)) + "</div>\n")
		content.WriteString(s.rows(kids[d.ID]) + s.related(d, kids[d.ID]) + s.prevNext(docs, i))
		content.WriteString("</section>\n")
	}
	banner := ""
	if s.Full {
		banner = `<p class="banner">` + s.t("Full build: includes hidden documents and notes. Do not publish.") + `</p>`
	}
	brandCSS, brandHead, brandPrint, brandFoot := s.brand()
	out := strings.NewReplacer(
		"{{TITLE}}", esc(title),
		"{{CSS}}", htmlCSS+brandCSS,
		"{{BRAND_HEAD}}", brandHead,
		"{{BRAND_PRINT}}", brandPrint,
		"{{BRAND_FOOT}}", brandFoot,
		"{{JS}}", s.labels(htmlJS, true),
		"{{BANNER}}", banner,
		"{{NAV}}", s.navHTML(),
		"{{CONTENT}}", content.String(),
	).Replace(s.labels(htmlTemplate, false))
	return lf(out)
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
// for line breaks) and the anchors <a id="x"> and </a> that mark field targets.
type brOnly struct{}

var anchorOpen = regexp.MustCompile(`^<a id="[A-Za-z0-9_.-]+">$`)

func (brOnly) RegisterFuncs(reg renderer.NodeRendererFuncRegisterer) {
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
