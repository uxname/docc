package main

import (
	"strings"
	"testing"
)

// DOC109 is a core rule: every document needs a kind with a schema, and the
// rule cannot be switched off. A kind schema without a boolean top-level
// additionalProperties is a config error.
func TestKindSchemaRequired(t *testing.T) {
	files := map[string]string{
		"docs/src/index.json": `{"title":"I","body":"See [[a]]."}`,
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
	files["docs/src/index.json"] = `{"title":"I","kind":"page","body":"See [[a]]."}`
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
