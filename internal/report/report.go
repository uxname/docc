// Package report prints diagnostics as text or JSON.
package report

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"

	"github.com/santiment/docc/internal/diag"
)

// Counts returns the number of errors, warnings and files with diagnostics.
func Counts(ds []diag.Diagnostic) (errors, warnings, files int) {
	seen := map[string]bool{}
	for i := range ds {
		switch ds[i].Lvl {
		case diag.Error:
			errors++
		case diag.Warn:
			warnings++
		}
		seen[ds[i].File] = true
	}
	return errors, warnings, len(seen)
}

// Text writes `file:line:col: level CODE message`, a hint line per
// diagnostic, a truncation banner when max > 0 cuts the list, and a summary.
func Text(w io.Writer, ds []diag.Diagnostic, max int) error {
	var b strings.Builder
	shown := ds
	if max > 0 && len(ds) > max {
		shown = ds[:max]
	}
	for i := range shown {
		d := &shown[i]
		fmt.Fprintf(&b, "%s:%d:%d: %s %s %s\n", d.File, d.Line, d.Column, d.Level, d.Code, d.Message)
		if d.Hint != "" {
			fmt.Fprintf(&b, "  hint: %s\n", d.Hint)
		}
	}
	if len(shown) < len(ds) {
		fmt.Fprintf(&b, "показано %d из %d\n", len(shown), len(ds))
	}
	e, wn, f := Counts(ds)
	fmt.Fprintf(&b, "%d errors, %d warnings in %d files\n", e, wn, f)
	_, err := io.WriteString(w, b.String())
	return err
}

// JSON writes the diagnostics as an array (empty: []), without a limit.
func JSON(w io.Writer, ds []diag.Diagnostic) error {
	if ds == nil {
		ds = []diag.Diagnostic{}
	}
	b, err := json.MarshalIndent(ds, "", "  ")
	if err != nil {
		return err
	}
	_, err = fmt.Fprintf(w, "%s\n", b)
	return err
}
