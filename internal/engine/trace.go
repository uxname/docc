package engine

import (
	"fmt"
	"slices"

	"github.com/uxname/docc/internal/loader"
	"github.com/uxname/docc/internal/trace"
)

// checkTrace reads the docc: tags in code: DOC401 for a malformed tag or
// region and for a target that does not resolve, DOC403 for a document of a
// covered kind that no tag points to. It runs only when trace.scan is set.
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
	p.Covers, p.Code, p.codeAt = map[string][]string{}, map[trace.Key]string{}, map[trace.Key][2]int{}
	bad := func(t trace.Tag, msg, hint string) {
		p.emitFile(t.File, "DOC401", t.Line, t.Col, "bad code tag: "+msg, hint)
	}
	for _, t := range tags {
		if t.Err != "" {
			bad(t, t.Err, "Close every docc:begin with docc:end; regions do not nest.")
			continue
		}
		if len(t.Targets) == 0 {
			bad(t, "no targets after docc:", "Write // docc: id, id#field or id#^anchor.")
		}
		for _, g := range t.Targets {
			id, field, hasField, ok := splitRef(g)
			if !ok {
				bad(t, fmt.Sprintf("%q is not a target", g), "Write id, id#field/section or id#^anchor, and separate targets with commas.")
				continue
			}
			d := p.Graph[id]
			if d == nil {
				bad(t, fmt.Sprintf("%q points to a document that does not exist", g), fmt.Sprintf("Create %s/%s.json or fix the id in the tag.", p.Cfg.Src, id))
				continue
			}
			if hasField && d.Parsed() {
				if msg, hint := fragmentProblem(d, id, field); msg != "" {
					bad(t, msg, hint)
					continue
				}
			}
			k := trace.Key{File: t.File, Target: g}
			if _, ok := p.codeAt[k]; !ok {
				p.codeAt[k] = [2]int{t.Line, t.Col}
			}
			p.Code[k] = ""
			if !slices.Contains(p.Covers[id], t.File) {
				p.Covers[id] = append(p.Covers[id], t.File)
			}
		}
	}
	for k, h := range trace.Hashes(tags) {
		if _, ok := p.Code[k]; ok {
			p.Code[k] = h
		}
	}
	for _, files := range p.Covers {
		slices.Sort(files)
	}
	for _, d := range p.Docs {
		if p.NeedsCode(d) {
			k := d.Root.Get("kind")
			p.emit(d, "DOC403", k.Start, k.End, fmt.Sprintf("no code tag points to %q, and kind %q expects one", d.ID, k.Str),
				fmt.Sprintf("Add // docc: %s to the code that implements it.", d.ID))
		}
	}
}

// NeedsCode reports whether d is a visible document whose kind schema sets
// "x-docc-covered" and no code tag points to it; false without trace.scan.
func (p *Project) NeedsCode(d *Doc) bool {
	if p.Covers == nil || !d.Parsed() || !p.inGraph(d) || d.Hidden || len(p.Covers[d.ID]) > 0 {
		return false
	}
	k := d.Root.Get("kind")
	if k == nil || k.Kind != loader.String {
		return false
	}
	kd := p.Schemas.Kind(k.Str)
	return kd != nil && kd.Covered
}
