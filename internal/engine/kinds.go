package engine

import (
	"fmt"
	"slices"
	"strings"

	"github.com/santiment/docc/internal/loader"
)

// checkKinds emits DOC106 (kind schema), DOC107 (registries) and DOC109 (no kind schema).
func (p *Project) checkKinds() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		k := d.Root.Get("kind")
		if k == nil {
			p.emit(d, "DOC109", d.Root.Start, d.Root.Start+1, "the document has no kind",
				`Add "kind" with the name of a schema from docs/schemas, or switch DOC109 off in docc.json.`)
		}
		if k != nil && k.Kind == loader.String {
			if !p.Schemas.HasKind(k.Str) {
				p.emit(d, "DOC109", k.Start, k.End, fmt.Sprintf("there is no schema for kind %q", k.Str),
					fmt.Sprintf("Create %s/%s.json, use a kind that has a schema, or switch DOC109 off in docc.json.", p.Cfg.Schemas, k.Str))
			}
			if p.Cfg.HasKinds && !slices.Contains(p.Cfg.Kinds, k.Str) {
				p.emit(d, "DOC107", k.Start, k.End, fmt.Sprintf("kind %q is not in the kinds registry", k.Str),
					registryHint("kinds", p.Cfg.Kinds, p.Cfg.File))
			}
			if !p.Off("DOC106") && p.Schemas.HasKind(k.Str) {
				for _, v := range p.Schemas.CheckKind(k.Str, d.Root) {
					p.emit(d, "DOC106", v.Start, v.End,
						fmt.Sprintf("document violates the schema of kind %q: %s", k.Str, v.Message), v.Hint)
				}
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

func registryHint(name string, allowed []string, file string) string {
	if len(allowed) == 0 {
		return fmt.Sprintf("The %s registry in %s is empty: no values are allowed.", name, file)
	}
	return fmt.Sprintf("Use one of: %s; or add the value to %q in %s.", strings.Join(allowed, ", "), name, file)
}
