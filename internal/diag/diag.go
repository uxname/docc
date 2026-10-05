// Package diag defines the diagnostic record shared by all checks and reports.
package diag

import "sort"

type Level uint8

// Levels are ordered so that sorting ascending puts errors first.
const (
	Error Level = iota
	Warn
	Off
)

func (l Level) String() string {
	switch l {
	case Error:
		return "error"
	case Warn:
		return "warn"
	}
	return "off"
}

// Diagnostic is one finding. Fields below the JSON block are internal.
type Diagnostic struct {
	Code      string `json:"code"`
	Rule      string `json:"rule"`
	Level     string `json:"level"`
	File      string `json:"file"`
	Line      int    `json:"line"`
	Column    int    `json:"column"`
	EndLine   int    `json:"endLine"`
	EndColumn int    `json:"endColumn"`
	Message   string `json:"message"`
	Hint      string `json:"hint"`

	Lvl        Level  `json:"-"`
	Start, End int    `json:"-"` // byte offsets in the source file; meaningless when NoPos
	NoPos      bool   `json:"-"` // no position in the JSON text (reported at 1:1)
	DocID      string `json:"-"` // owner document id ("" for config/code/out files)
}

// Sort orders diagnostics deterministically: level, file path (bytes), line,
// column, code, rule name.
func Sort(ds []Diagnostic) {
	sort.SliceStable(ds, func(i, j int) bool {
		a, b := &ds[i], &ds[j]
		if a.Lvl != b.Lvl {
			return a.Lvl < b.Lvl
		}
		if a.File != b.File {
			return a.File < b.File
		}
		if a.Line != b.Line {
			return a.Line < b.Line
		}
		if a.Column != b.Column {
			return a.Column < b.Column
		}
		if a.Code != b.Code {
			return a.Code < b.Code
		}
		return a.Rule < b.Rule
	})
}
