package main

import (
	"strings"
	"testing"
)

func TestBrokenFileStaysANode(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json":  `{"title":"I","body":"See [[broken]]."}`,
		"docs/src/broken.json": `{`,
		"docs/src/x.json":      `{"title":"X","parent":"broken"}`,
	})
	_, out, _ := do("validate", "--format", "json")
	if strings.Contains(out, "DOC203") || !strings.Contains(out, "DOC101") {
		t.Fatalf("x is reachable through the broken node:\n%s", out)
	}
}

func TestTraceIgnoresBOMAndConfigPositions(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I"}`,
		"docc.json":           `{"trace":{"scan":["tests/**"]}}`,
		"tests/a.txt":         "\xEF\xBB\xBF// @covers nope\r\n",
	})
	if _, out, _ := do("validate"); !strings.Contains(out, "tests/a.txt:1:4: error DOC401") {
		t.Fatalf("%s", out)
	}
	for in, want := range map[string]string{
		`{"customRules":[{"id":"custom/x","level":"warn","given":"a","forbid":"x","message":"m"}]}`: "docc.json:1:",
		`{"rules":{"DOC301":{"words":["бы*стро"]}}}`:                                                "docc.json:1:",
	} {
		project(t, map[string]string{"docs/src/index.json": `{"title":"I"}`, "docc.json": in})
		if code, _, errs := do("validate"); code != 2 || !strings.HasPrefix(errs, "docc: error: "+want) {
			t.Errorf("%s: %d %q", in, code, errs)
		}
	}
}

func TestCoversWithoutComma(t *testing.T) {
	project(t, map[string]string{
		"docc.json":           `{"trace":{"scan":["*.go"]}}`,
		"docs/src/index.json": `{"title":"I","body":"See [[a]] and [[b]]."}`,
		"docs/src/a.json":     `{"title":"A"}`,
		"docs/src/b.json":     `{"title":"B"}`,
		"x.go":                "// @covers a b\n// @covers a, b\n",
	})
	_, out, _ := do("validate", "--format", "json")
	if strings.Count(out, `"DOC401"`) != 1 || !strings.Contains(out, "Separate ids with commas") {
		t.Fatalf("only the tag without a comma is reported:\n%s", out)
	}
}

func TestBrokenParentNoCascade(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I"}`,
		"docs/src/n.json":     `{"title":"N","parent":5}`,
		"docs/src/s.json":     `{"title":"S","parent":"s"}`,
	})
	_, out, _ := do("validate", "--format", "json")
	for _, bad := range []string{"DOC203"} {
		if strings.Contains(out, bad) {
			t.Fatalf("%s is a cascade here:\n%s", bad, out)
		}
	}
	if !strings.Contains(out, "DOC105") || strings.Count(out, `"DOC204"`) != 1 {
		t.Fatalf("one signal each:\n%s", out)
	}
}

func TestSyntaxHints(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"Title":"I"}`,
		"docs/src/e.json":     ``,
		"docs/src/a.json":     `[1]`,
	})
	_, out, _ := do("validate", "--format", "json")
	for _, w := range []string{`is named \"title\"`, "file is empty", "must be an object"} {
		if !strings.Contains(out, w) {
			t.Fatalf("missing %q:\n%s", w, out)
		}
	}
}
