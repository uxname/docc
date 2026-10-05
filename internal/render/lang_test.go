package render

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// Every label of the output has a Russian entry, and every entry is used: a
// label renamed in the code but not in ru fails both ways.
func TestLabelsTranslated(t *testing.T) {
	used := map[string]bool{}
	for _, k := range append(Labels(htmlTemplate), Labels(htmlJS)...) {
		used[k] = true
	}
	call := regexp.MustCompile(`(?:\.t|\bgroup|\badd)\("([^"]+)"`)
	files, _ := filepath.Glob("*.go")
	for _, f := range files {
		if strings.HasSuffix(f, "_test.go") {
			continue
		}
		src, err := os.ReadFile(f)
		if err != nil {
			t.Fatal(err)
		}
		for _, m := range call.FindAllStringSubmatch(string(src), -1) {
			used[m[1]] = true
		}
	}
	if len(used) < 30 {
		t.Fatalf("found only %d labels, the scan is broken", len(used))
	}
	for k := range used {
		if _, ok := ru[k]; !ok {
			t.Errorf("label %q has no Russian entry in ru", k)
		}
	}
	for k := range ru {
		if !used[k] {
			t.Errorf("ru entry %q is not used by any label", k)
		}
	}
}
