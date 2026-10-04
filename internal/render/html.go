package render

import (
	"bytes"
	_ "embed"
	"html"
	"strconv"
	"strings"

	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/extension"
	"github.com/yuin/goldmark/parser"
	"github.com/yuin/goldmark/renderer"
	"github.com/yuin/goldmark/util"
)

//go:embed page.html
var htmlTemplate string

// HTML returns every page in one self-contained HTML file: a document tree
// with search on the left, the pages in tree order on the right. Same call
// contract as Files.
func (r *Renderer) HTML() string {
	s := *r
	s.single = true // in-file links; the headings get their ids from the converter
	md := goldmark.New(
		goldmark.WithExtensions(extension.GFM), // as on GitHub: tables, autolinks, strikethrough
		goldmark.WithParserOptions(parser.WithAutoHeadingID()),
		// no WithUnsafe: raw HTML written by agents is dropped, only <br> survives
		goldmark.WithRendererOptions(renderer.WithNodeRenderers(util.Prioritized(brOnly{}, 100))),
	)
	conv := func(prefix, src string) string {
		var b bytes.Buffer
		ctx := parser.NewContext(parser.WithIDs(&ids{prefix: prefix, used: map[string]bool{}}))
		if err := md.Convert([]byte(src), &b, parser.WithContext(ctx)); err != nil {
			return html.EscapeString(src) // goldmark does not fail on a string; keep the text anyway
		}
		return b.String()
	}
	title := "docc"
	var content strings.Builder
	for i, d := range s.ordered() {
		if i == 0 {
			title = titleOf(d)
		}
		content.WriteString(`<section id="` + html.EscapeString(d.ID) + "\">\n" + conv(d.ID, s.page(d)) + "</section>\n")
	}
	out := strings.NewReplacer(
		"{{TITLE}}", html.EscapeString(title),
		"{{NAV}}", conv("nav", strings.Join(s.treeBlocks(), "\n\n")),
		"{{CONTENT}}", content.String(),
	).Replace(htmlTemplate)
	return lf(out)
}

// ids gives heading ids <prefix>--<anchor>: the page id plus the same anchor
// that [[id#field]] links use, unique within the page.
type ids struct {
	prefix string
	used   map[string]bool
}

func (g *ids) Generate(value []byte, _ ast.NodeKind) []byte {
	base := g.prefix + "--" + anchor(string(value))
	id := base
	for n := 1; g.used[id]; n++ {
		id = base + "-" + strconv.Itoa(n)
	}
	g.used[id] = true
	return []byte(id)
}

func (g *ids) Put(value []byte) { g.used[string(value)] = true }

// brOnly renders raw inline HTML as nothing except <br>, which table cells use
// for line breaks.
type brOnly struct{}

func (brOnly) RegisterFuncs(reg renderer.NodeRendererFuncRegisterer) {
	reg.Register(ast.KindRawHTML, func(w util.BufWriter, src []byte, n ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			var tag []byte
			raw := n.(*ast.RawHTML)
			for i := 0; i < raw.Segments.Len(); i++ {
				seg := raw.Segments.At(i)
				tag = append(tag, seg.Value(src)...)
			}
			switch strings.ToLower(strings.ReplaceAll(string(tag), " ", "")) {
			case "<br>", "<br/>":
				_, _ = w.WriteString("<br>")
			default:
				_, _ = w.WriteString("<!-- raw HTML omitted -->")
			}
		}
		return ast.WalkSkipChildren, nil
	})
}
