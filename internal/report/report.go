// Package report prints diagnostics as text or JSON.
package report

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"

	"github.com/uxname/docc/internal/diag"
)

// Counts returns the number of errors and warnings.
func Counts(ds []diag.Diagnostic) (errors, warnings int) {
	for i := range ds {
		switch ds[i].Lvl {
		case diag.Error:
			errors++
		case diag.Warn:
			warnings++
		}
	}
	return errors, warnings
}

func plural(n int, one string) string {
	if n == 1 {
		return fmt.Sprintf("%d %s", n, one)
	}
	return fmt.Sprintf("%d %ss", n, one)
}

// limit cuts ds to max items; max <= 0 means no limit.
func limit(ds []diag.Diagnostic, max int) []diag.Diagnostic {
	if max > 0 && len(ds) > max {
		return ds[:max]
	}
	return ds
}

// Text writes `file:line:col: level CODE (rule) message`, a hint line per
// diagnostic, a truncation banner when max > 0 cuts the list, and a summary
// of the checked files.
func Text(w io.Writer, ds []diag.Diagnostic, max, checked int) error {
	var b strings.Builder
	shown := limit(ds, max)
	for i := range shown {
		d := &shown[i]
		fmt.Fprintf(&b, "%s:%d:%d: %s %s (%s) %s\n", d.File, d.Line, d.Column, d.Level, d.Code, d.Rule, d.Message)
		if d.Hint != "" {
			fmt.Fprintf(&b, "  hint: %s\n", d.Hint)
		}
	}
	if len(shown) < len(ds) {
		fmt.Fprintf(&b, "showing %d of %d\n", len(shown), len(ds))
	}
	e, wn := Counts(ds)
	fmt.Fprintf(&b, "checked %s: %s, %s\n", plural(checked, "file"), plural(e, "error"), plural(wn, "warning"))
	if len(ds) > 0 {
		b.WriteString("Run docc explain <rule> for details\n")
	}
	_, err := io.WriteString(w, b.String())
	return err
}

// Summary is the "summary" object of the JSON report.
type Summary struct {
	Checked   int    `json:"checked"`
	Errors    int    `json:"errors"`
	Warnings  int    `json:"warnings"`
	Shown     int    `json:"shown"`
	Truncated bool   `json:"truncated"`
	Failure   string `json:"failure,omitempty"` // why the exit code is 1 without errors
}

// JSON writes {"summary": ..., "diagnostics": [...]}; max > 0 cuts the list.
func JSON(w io.Writer, ds []diag.Diagnostic, max, checked int, failure string) error {
	shown := limit(ds, max)
	if shown == nil {
		shown = []diag.Diagnostic{}
	}
	e, wn := Counts(ds)
	out := struct {
		Summary     Summary           `json:"summary"`
		Diagnostics []diag.Diagnostic `json:"diagnostics"`
	}{Summary{checked, e, wn, len(shown), len(shown) < len(ds), failure}, shown}
	enc := json.NewEncoder(w)
	enc.SetEscapeHTML(false) // "<", ">" and "&" stay readable in messages
	enc.SetIndent("", "  ")
	return enc.Encode(out)
}
