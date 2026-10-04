package engine

import (
	"fmt"
	"slices"
	"strings"

	"github.com/santiment/docc/internal/loader"
)

// checkKinds emits DOC106 (kind schema) and DOC107 (registries).
func (p *Project) checkKinds() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		if k := d.Root.Get("kind"); k != nil && k.Kind == loader.String {
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
