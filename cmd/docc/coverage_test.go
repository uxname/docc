package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/santiment/docc/internal/rules"
)

// Every code has a red and a green golden case (the definition of done).
func TestEveryRuleHasGoldenCases(t *testing.T) {
	root, _ := filepath.Abs("../../testdata")
	for _, r := range rules.All() {
		for _, kind := range []string{"red", "green"} {
			dir := filepath.Join(root, r.Code, kind)
			if _, err := os.Stat(filepath.Join(dir, "expected.json")); err != nil {
				t.Errorf("%s: no %s case with expected.json", r.Code, kind)
			}
		}
	}
}

// Positions inside strings with escapes and in multi-line values.
func TestPositionsWithEscapesAndNewlines(t *testing.T) {
	bs := "\\"
	// body: "line1" LF "line2 fast" with an escaped cyrillic letter before the word
	doc := `{"title":"I","body":"Первая строка` + bs + `nвторая ` + bs + `u0430 fast` + bs + `n` + bs + `ttail"}`
	project(t, map[string]string{"docs/src/index.json": doc})
	_, out, _ := do("validate", "--format", "json")
	var ds []struct {
		Code                             string
		Line, Column, EndLine, EndColumn int
	}
	if err := json.Unmarshal(diagnosticsOf(out), &ds); err != nil || len(ds) != 1 || ds[0].Code != "DOC301" {
		t.Fatalf("%v %s", err, out)
	}
	// source column of "fast": the file is one line; count runes before it
	idx := strings.Index(doc, "fast")
	want := len([]rune(doc[:idx])) + 1
	if ds[0].Line != 1 || ds[0].Column != want || ds[0].EndColumn != want+4 || ds[0].EndLine != 1 {
		t.Fatalf("got %d:%d-%d:%d, want 1:%d-1:%d", ds[0].Line, ds[0].Column, ds[0].EndLine, ds[0].EndColumn, want, want+4)
	}
	// a pretty-printed file: the diagnostic is on line 3
	pretty := "{\n  \"title\": \"I\",\n  \"body\": \"x fast\"\n}\n"
	project(t, map[string]string{"docs/src/index.json": pretty})
	_, out, _ = do("validate", "--format", "json")
	if err := json.Unmarshal(diagnosticsOf(out), &ds); err != nil || ds[0].Line != 3 || ds[0].Column != 14 {
		t.Fatalf("%v %+v", err, ds)
	}
	// a diagnostic that spans lines: a sentence over the limit across escaped newlines
	multi := "{\n  \"title\": \"I\",\n  \"body\": \"one two three" + bs + "nfour five six\"\n}\n"
	project(t, map[string]string{
		"docs/src/index.json": multi,
		"docc.json":           `{"rules":{"DOC351":{"level":"warn","max":3}}}`,
	})
	_, out, _ = do("validate", "--format", "json")
	if err := json.Unmarshal(diagnosticsOf(out), &ds); err != nil || len(ds) != 1 || ds[0].Line != 3 || ds[0].EndLine != 3 {
		t.Fatalf("%v %+v", err, ds)
	}
	// real line breaks inside the JSON text are not allowed in strings; the value may span lines only via \n
	if ds[0].EndColumn <= ds[0].Column {
		t.Fatalf("range must have a width: %+v", ds[0])
	}
}

// The skill must not mention rules or commands that do not exist.
func TestSkillMatchesRegistryAndCommands(t *testing.T) {
	b, err := os.ReadFile("../../skills/docc/SKILL.md")
	if err != nil {
		t.Fatal(err)
	}
	text := string(b)
	for _, m := range regexp.MustCompile(`DOC\d{3}`).FindAllString(text, -1) {
		if rules.ByCode(m) == nil {
			t.Errorf("skill mentions unknown code %s", m)
		}
	}
	for _, cmd := range regexp.MustCompile("`docc ([a-z]+)").FindAllStringSubmatch(text, -1) {
		switch cmd[1] {
		case "validate", "build", "check", "explain", "rules", "schema", "init":
		default:
			t.Errorf("skill mentions unknown command docc %s", cmd[1])
		}
	}
}
