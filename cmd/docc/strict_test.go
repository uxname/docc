package main

import (
	"regexp"
	"strings"
	"testing"
)

// DOC109 is a core rule: every document needs a kind with a schema, and the
// rule cannot be switched off. A kind schema without a boolean top-level
// additionalProperties is a config error.
func TestKindSchemaRequired(t *testing.T) {
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
	if code, _, errs := do("validate"); code != 2 || !strings.Contains(errs, "DOC109 is a core rule") {
		t.Fatalf("off: %d %s", code, errs)
	}
	delete(files, "docc.json")
	files["docs/schemas/page.json"] = `{"type":"object","required":["title"],"additionalProperties":true}`
	files["docs/schemas/ghost.json"] = `{"type":"object","required":["title"]}`
	files["docs/src/index.json"] = `{"title":"I","kind":"page","links":["a"]}`
	rawProject(t, files)
	if code, _, errs := do("validate"); code != 2 || strings.Count(errs, "additionalProperties") != 1 || !strings.Contains(errs, "docs/schemas/ghost.json") {
		t.Fatalf("no additionalProperties: %d %s", code, errs)
	}
	files["docs/schemas/ghost.json"] = `{"type":"object","required":["title"],"additionalProperties":false}`
	rawProject(t, files)
	if code, out, _ := do("validate"); code != 0 {
		t.Fatalf("with schemas: %d %s", code, out)
	}
}

// DOC209 is a warning by default; the fingerprint from its hint confirms the link.
func TestSuspectLinkIsWarnedByDefault(t *testing.T) {
	files := map[string]string{
		"docs/schemas/page.json": `{"additionalProperties":true}`,
		"docs/src/index.json":    `{"title":"I","kind":"page","links":["a"]}`,
		"docs/src/a.json":        `{"title":"A","kind":"page","parent":"index"}`,
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
	files["docs/src/index.json"] = `{"title":"I","kind":"page","links":["a"],"reviewed":{"a":"` + fp[1] + `"}}`
	rawProject(t, files)
	if code, out, _ := do("validate"); code != 0 || strings.Contains(out, "DOC209") {
		t.Fatalf("confirmed link must be quiet: %d %s", code, out)
	}
}
