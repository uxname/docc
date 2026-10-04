package engine

import (
	"strings"

	"github.com/santiment/docc/internal/loader"
)

// checkSyntax runs the per-document structure checks: DOC103 and DOC105.
// (DOC101, DOC102 and DOC104 are emitted while loading.)
func (p *Project) checkSyntax() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		t := d.Root.Get("title")
		switch {
		case t == nil:
			p.emit(d, "DOC103", d.Root.Start, d.Root.Start+1, "the document has no title",
				`Add a non-empty "title" string.`)
		case t.Kind == loader.String && strings.TrimSpace(t.Str) == "":
			p.emit(d, "DOC103", t.Start, t.End, "the title is empty",
				"Write a short, meaningful title.")
		}
		for _, v := range p.Schemas.CheckBuiltin(d.Root) {
			p.emit(d, "DOC105", v.Start, v.End, "document does not match the built-in schema: "+v.Message, v.Hint)
		}
	}
}
