// Package fixes applies diagnostic fixes as patches by source offsets; all
// other formatting of the file stays untouched.
package fixes

import (
	"bytes"
	"fmt"
	"os"
	"sort"

	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/loader"
)

// Apply applies edits to data. Edits that overlap an earlier one (or insert at
// the same place) are skipped; the number applied is returned.
func Apply(data []byte, edits []diag.Edit) ([]byte, int) {
	sorted := append([]diag.Edit(nil), edits...)
	sort.SliceStable(sorted, func(i, j int) bool {
		if sorted[i].Start != sorted[j].Start {
			return sorted[i].Start < sorted[j].Start
		}
		return sorted[i].End < sorted[j].End
	})
	var out bytes.Buffer
	pos, applied := 0, 0
	lastInsert := -1
	for _, e := range sorted {
		if e.Start < pos || e.End > len(data) || e.Start > e.End {
			continue
		}
		if e.Start == e.End && e.Start == lastInsert {
			continue
		}
		out.Write(data[pos:e.Start])
		out.WriteString(e.Text)
		pos = e.End
		if e.Start == e.End {
			lastInsert = e.Start
		}
		applied++
	}
	out.Write(data[pos:])
	return out.Bytes(), applied
}

// Run fixes the project in place until no fix applies (a fix can enable the
// next one, for example the first added link creates the links field).
// With unsafe, fixes of kind unsafe are applied too. It returns the number of
// edits and files changed.
func Run(load func() (*engine.Project, error), unsafe bool) (edits, files int, err error) {
	changed := map[string]bool{}
	for pass := 0; pass < 20; pass++ {
		p, err := load()
		if err != nil {
			return edits, len(changed), err
		}
		ds := p.Validate()
		byFile := map[string][]diag.Edit{}
		for i := range ds {
			d := &ds[i]
			allowed := d.Fixable == diag.FixSafe || (unsafe && d.Fixable == diag.FixUnsafe)
			if d.Fix == nil || !allowed {
				continue
			}
			byFile[d.File] = append(byFile[d.File], d.Fix.Edits...)
		}
		if len(byFile) == 0 {
			return edits, len(changed), nil
		}
		names := make([]string, 0, len(byFile))
		for f := range byFile {
			names = append(names, f)
		}
		sort.Strings(names)
		progress := false
		for _, f := range names {
			n, err := fixFile(p, f, byFile[f])
			if err != nil {
				return edits, len(changed), err
			}
			if n > 0 {
				edits += n
				changed[f] = true
				progress = true
			}
		}
		if !progress {
			return edits, len(changed), nil
		}
	}
	return edits, len(changed), fmt.Errorf("fixes did not settle after 20 passes")
}

func fixFile(p *engine.Project, file string, edits []diag.Edit) (int, error) {
	abs := p.Cfg.Abs(file)
	raw, err := os.ReadFile(abs)
	if err != nil {
		return 0, err
	}
	bom := 0
	if bytes.HasPrefix(raw, []byte{0xEF, 0xBB, 0xBF}) {
		bom = 3
	}
	body, n := Apply(raw[bom:], edits)
	if n == 0 {
		return 0, nil
	}
	if r := loader.Parse(body); r.Err != nil {
		return 0, fmt.Errorf("internal error: a fix would break %s (%s); the file was not changed", file, r.Err.Msg)
	}
	return n, os.WriteFile(abs, append(raw[:bom:bom], body...), 0o644)
}
