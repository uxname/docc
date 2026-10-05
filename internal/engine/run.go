package engine

import (
	"strings"

	"github.com/santiment/docc/internal/diag"
)

// Validate runs every check and returns the diagnostics, sorted.
func (p *Project) Validate() []diag.Diagnostic {
	p.checkSyntax()
	p.analyze()
	p.checkGraph()
	p.parseIgnores()
	p.checkKinds()
	p.checkText()
	p.checkMarkdown()
	p.checkCustom()
	p.checkTrace()
	p.checkLock()
	p.suppress()
	diag.Sort(p.Diags)
	return p.Diags
}

// Structural reports whether the diagnostics contain an error of group 1xx
// or 2xx; build and check stop on those.
func Structural(ds []diag.Diagnostic) bool {
	for i := range ds {
		if ds[i].Lvl == diag.Error && (strings.HasPrefix(ds[i].Code, "DOC1") || strings.HasPrefix(ds[i].Code, "DOC2")) {
			return true
		}
	}
	return false
}
