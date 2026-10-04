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
	p.Covers = map[string][]string{}
	for _, t := range tags {
		if _, ok := p.Graph[t.Next]; ok && t.Next != "" {
			p.emitFile(t.File, "DOC401", t.Line, t.Col, fmt.Sprintf("@covers lists ids without a comma: %q follows the ids but is not checked", t.Next),
				"Separate ids with commas: @covers a, b.")
		}
		for _, id := range t.IDs {
			if _, ok := p.Graph[id]; ok {
				p.Covers[id] = append(p.Covers[id], fmt.Sprintf("%s:%d", t.File, t.Line))
			} else {
				p.emitFile(t.File, "DOC401", t.Line, t.Col, fmt.Sprintf("@covers points to a document that does not exist: %q", id),
					fmt.Sprintf("Create %s/%s.json or fix the id in the tag.", p.Cfg.Src, id))
			}
		}
	}
}
