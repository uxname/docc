package engine

import (
	"fmt"

	"github.com/santiment/docc/internal/trace"
)

// checkTrace emits DOC401 for @covers tags that name unknown documents.
// It runs only when trace.scan is set.
func (p *Project) checkTrace() {
	if len(p.Cfg.TraceScan) == 0 {
		return
	}
	tags, err := trace.Scan(p.Cfg.Root, p.Cfg.TraceScan, []string{
		p.Cfg.Abs(p.Cfg.Src), p.Cfg.Abs(p.Cfg.Out), p.Cfg.Abs(p.Cfg.OutFull),
	})
	if err != nil {
		return
	}
	for _, t := range tags {
		for _, id := range t.IDs {
			if _, ok := p.Graph[id]; !ok {
				p.emitFile(t.File, "DOC401", t.Line, t.Col, fmt.Sprintf("@covers points to a document that does not exist: %q", id),
					fmt.Sprintf("Create %s/%s.json or fix the id in the tag.", p.Cfg.Src, id))
			}
		}
	}
}
