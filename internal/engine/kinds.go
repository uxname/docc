package engine

import (
	"fmt"
	"regexp"
	"slices"
	"strings"

	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/schema"
)

// checkKinds emits DOC106 (kind schema), DOC107 (tags registry), DOC109 (no
// kind schema) and DOC214 (reference to a document of a wrong kind).
func (p *Project) checkKinds() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		k := d.Root.Get("kind")
		if k == nil {
			p.emit(d, "DOC109", d.Root.Start, d.Root.Start+1, "the document has no kind",
				fmt.Sprintf(`Add "kind" with the name of a schema from %s.`, p.Cfg.Schemas))
		}
		if k != nil && k.Kind == loader.String && kindName.MatchString(k.Str) { // a malformed kind is DOC105
			if !p.Schemas.HasKind(k.Str) {
				p.emit(d, "DOC109", k.Start, k.End, fmt.Sprintf("there is no schema for kind %q", k.Str),
					fmt.Sprintf("Create %s/%s.json or use a kind that has a schema.", p.Cfg.Schemas, k.Str))
			}
			if !p.Off("DOC106") && p.Schemas.HasKind(k.Str) {
				for _, v := range p.Schemas.CheckKind(k.Str, d.Root) {
					p.emit(d, "DOC106", v.Start, v.End,
						fmt.Sprintf("document violates the schema of kind %q: %s", k.Str, v.Message), v.Hint)
				}
			}
			if kd := p.Schemas.Kind(k.Str); kd != nil && p.inGraph(d) {
				p.wrongTargets(d, k.Str, kd)
			}
		}
		if p.Cfg.HasTags {
			for _, t := range StringList(d.Root.Get("tags")) {
				if !slices.Contains(p.Cfg.Tags, t.Str) {
					p.emit(d, "DOC107", t.Start, t.End, fmt.Sprintf("tag %q is not in the tags registry", t.Str),
						registryHint("tags", p.Cfg.Tags, p.Cfg.File))
				}
			}
		}
	}
}

// wrongTargets emits DOC214: the parent or a reference in a field whose
// schema property has "x-docc-target" points to a document of another kind.
// A missing target or one without a kind is left to DOC202 and DOC109.
func (p *Project) wrongTargets(d *Doc, kindName string, k *schema.Kind) {
	check := func(field, id string, start, end int, what string) {
		want := k.Targets(field)
		t := p.Graph[id]
		if len(want) == 0 || t == nil || !t.Parsed() {
			return
		}
		tk := t.Root.Get("kind")
		if tk == nil || tk.Kind != loader.String || slices.Contains(want, tk.Str) {
			return
		}
		p.emit(d, "DOC214", start, end,
			fmt.Sprintf("%s points to %q of kind %q, but field %q takes only kind %s", what, id, tk.Str, field, strings.Join(want, ", ")),
			fmt.Sprintf("Point it to a document of kind %s, or change \"x-docc-target\" of %q in %s/%s.json.", strings.Join(want, " or "), field, p.Cfg.Schemas, kindName))
	}
	if n := parentOf(d); n != nil {
		check("parent", n.Str, n.Start, n.End, "parent")
	}
	for _, r := range d.Refs {
		if r.OK {
			field, _, _ := strings.Cut(strings.TrimPrefix(r.Ptr, "/"), "/")
			field = strings.ReplaceAll(strings.ReplaceAll(field, "~1", "/"), "~0", "~")
			check(field, r.Target, r.Start, r.End, "reference [["+r.Inner+"]]")
		}
	}
}

var kindName = regexp.MustCompile(`^[a-z0-9-]+$`)

func registryHint(name string, allowed []string, file string) string {
	if len(allowed) == 0 {
		return fmt.Sprintf("The %s registry in %s is empty: no values are allowed.", name, file)
	}
	return fmt.Sprintf("Use one of: %s; or add the value to %q in %s.", strings.Join(allowed, ", "), name, file)
}
