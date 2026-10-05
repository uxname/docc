package main

import (
	"regexp"
	"strings"
	"testing"
)

// DOC109 is on by default and is switched off in docc.json, for all files or for some.
func TestKindSchemaRequiredUnlessSwitchedOff(t *testing.T) {
	files := map[string]string{
		"docs/src/index.json": `{"title":"I","links":["a"]}`,
		"docs/src/a.json":     `{"title":"A","kind":"ghost"}`,
	}
	rawProject(t, files)
	if code, out, _ := do("validate"); code != 1 || strings.Count(out, "error DOC109") != 2 {
		t.Fatalf("default: want two DOC109 errors: %d %s", code, out)
	}
	files["docc.json"] = `{"rules":{"DOC109":"off"}}`
	rawProject(t, files)
	if code, out, _ := do("validate"); code != 0 || strings.Contains(out, "DOC109") {
		t.Fatalf("off: %d %s", code, out)
	}
	files["docc.json"] = `{"overrides":[{"include":["a.json"],"rules":{"DOC109":"off"}}]}`
	rawProject(t, files)
	if code, out, _ := do("validate"); code != 1 || strings.Count(out, "error DOC109") != 1 || !strings.Contains(out, "index.json:1:1") {
		t.Fatalf("override: only index.json must fail: %d %s", code, out)
	}
	files["docs/schemas/page.json"] = `{"type":"object","required":["title"]}`
	files["docs/schemas/ghost.json"] = `{"type":"object","required":["title"]}`
	files["docs/src/index.json"] = `{"title":"I","kind":"page","links":["a"]}`
	delete(files, "docc.json")
	rawProject(t, files)
	if code, out, _ := do("validate"); code != 0 {
		t.Fatalf("with schemas: %d %s", code, out)
	}
}

// DOC209 is a warning by default; the fingerprint from its hint confirms the link.
func TestSuspectLinkIsWarnedByDefault(t *testing.T) {
	files := map[string]string{
		"docc.json":           `{"rules":{"DOC109":"off"}}`,
		"docs/src/index.json": `{"title":"I","links":["a"]}`,
		"docs/src/a.json":     `{"title":"A","parent":"index"}`,
	}
	rawProject(t, files)
	code, out, _ := do("validate")
	if code != 0 || !strings.Contains(out, "DOC209") || !strings.Contains(out, "1 warning") {
		t.Fatalf("%d %s", code, out)
	}
	fp := regexp.MustCompile(`"a": "([0-9a-f]{8})"`).FindStringSubmatch(out)
	if fp == nil {
		t.Fatalf("no fingerprint in the hint: %s", out)
	}
	files["docs/src/index.json"] = `{"title":"I","links":["a"],"reviewed":{"a":"` + fp[1] + `"}}`
	rawProject(t, files)
	if code, out, _ := do("validate"); code != 0 || strings.Contains(out, "DOC209") {
		t.Fatalf("confirmed link must be quiet: %d %s", code, out)
	}
}
