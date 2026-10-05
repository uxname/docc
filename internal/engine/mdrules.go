package engine

import (
	"fmt"
	"strings"

	"github.com/santiment/docc/internal/diag"
)

// checkMarkdown runs DOC308-DOC312.
func (p *Project) checkMarkdown() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		res := p.Cfg.Resolve()
		on := func(code string) bool { return res.Rule(code).Level != diag.Off }
		strs := d.Strings()
		for i := range strs {
			sn := &strs[i]
			if !sn.IsText() {
				continue
			}
			sc := sn.Scan()
			s := sn.Node.Str
			if on("DOC308") && sc.UnclosedFence >= 0 {
				end := sc.UnclosedFence
				if nl := strings.IndexByte(s[end:], '\n'); nl >= 0 {
					end += nl
				} else {
					end = len(s)
				}
				a, b := d.Range(sn.Node, sc.UnclosedFence, end)
				p.emit(d, "DOC308", a, b, "a code fence is opened but never closed", "Close the block with a matching fence line.")
			}
			if on("DOC309") {
				for _, h := range sc.Headings() {
					n := 0
					for s[h.Start+n] == '#' {
						n++
					}
					a, b := d.Range(sn.Node, h.Start, h.Start+n)
					p.emit(d, "DOC309", a, b, "a heading line inside a field breaks the structure of the output",
						"Remove the leading # characters; the field name already becomes a heading.")
				}
			}
			if on("DOC310") {
				for _, tg := range sc.HTMLTags() {
					a, b := d.Range(sn.Node, tg.Start, tg.End)
					p.emit(d, "DOC310", a, b, fmt.Sprintf("raw HTML tag <%s> in the text", tg.Name),
						"Use Markdown instead of HTML; scripts are never allowed.")
				}
			}
		}
		if on("DOC312") && p.inGraph(d) {
			p.anchors(d)
		}
	}
}

// anchors implements DOC312 for [[id#field]] in text and notes.
func (p *Project) anchors(d *Doc) {
	for _, r := range d.Refs {
		if !r.OK || r.Field == "" {
			continue
		}
		t := p.Graph[r.Target]
		if t == nil || !t.Parsed() {
			continue // DOC202 or DOC101 speaks
		}
		name, block := strings.CutPrefix(r.Field, "^")
		switch {
		case block:
			if t.Blocks[name] == nil {
				p.emit(d, "DOC312", r.Start, r.End, fmt.Sprintf("document %q has no block anchor %q", r.Target, "^"+name),
					"End a paragraph or list item of the target with \" ^"+name+"\", or fix the name.")
			}
		case ServiceFields[r.Field]:
			p.emit(d, "DOC312", r.Start, r.End, fmt.Sprintf("%q is a service field and cannot be an anchor", r.Field),
				"Point to a content field such as body or a free field.")
		case Resolve(t.Root, r.Field) == nil:
			p.emit(d, "DOC312", r.Start, r.End, fmt.Sprintf("document %q has no field %q", r.Target, r.Field),
				"Use an existing top-level field or field/section.")
		}
	}
}
