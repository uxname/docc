package engine

import (
	"fmt"
	"maps"
	"slices"
	"strings"

	"github.com/uxname/docc/internal/loader"
	"github.com/uxname/docc/internal/schema"
)

// checkSyntax runs the per-document structure checks: DOC103, DOC105 and DOC110.
// (DOC101, DOC102 and DOC104 are emitted while loading.)
func (p *Project) checkSyntax() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		t := d.Root.Get("title")
		switch {
		case t == nil:
			hint := `Add a non-empty "title" string.`
			if d.Root.Get("Title") != nil {
				hint = `The field is named "title" (lowercase): rename "Title".`
			}
			p.emit(d, "DOC103", d.Root.Start, d.Root.Start+1, "the document has no title", hint)
		case t.Kind == loader.String && strings.TrimSpace(t.Str) == "":
			p.emit(d, "DOC103", t.Start, t.End, "the title is empty",
				"Write a short, meaningful title.")
		}
		for _, v := range p.Schemas.CheckBuiltin(d.Root) {
			p.emit(d, "DOC105", v.Start, v.End, "document does not match the built-in schema: "+v.Message, v.Hint)
		}
		p.reservedTypos(d)
	}
}

// reservedTypos reports DOC110: a free field whose name is one or two edits
// from a reserved key ("parnet", "hiden"), which would silently be prose.
func (p *Project) reservedTypos(d *Doc) {
	kind := d.Root.Get("kind")
	names := slices.Sorted(maps.Keys(reservedKeys))
	for _, m := range d.Root.Members {
		if reservedKeys[m.Key] || strings.EqualFold(m.Key, "title") { // a wrong-case title is DOC103
			continue
		}
		if kind != nil && p.Schemas.Declares(kind.Str, m.Key) {
			continue
		}
		c, dist := schema.Closest(m.Key, names)
		if c == "" || len([]rune(m.Key)) < 3 || (dist != 1 && !(dist == 2 && sameLetters(m.Key, c))) {
			continue // one edit, or two letters swapped; "other" or "3 notes" are prose fields
		}
		p.emit(d, "DOC110", m.KeyStart, m.KeyEnd, fmt.Sprintf("field %q looks like a misspelled %q", m.Key, c),
			fmt.Sprintf("Did you mean %q? Rename it, or declare %q in the schema of the kind if it is meant.", c, m.Key))
	}
}

// sameLetters reports whether a and b have the same letters in another order.
func sameLetters(a, b string) bool {
	x, y := []rune(a), []rune(b)
	slices.Sort(x)
	slices.Sort(y)
	return slices.Equal(x, y)
}
