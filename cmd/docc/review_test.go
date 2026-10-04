package main

import (
	"strings"
	"testing"
)

func TestFixableMatchesRealFix(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","links":["b"]}`,
		"docs/src/b.json":     `{"title":"B","parent":"index","body":"[[b]]"}`,
	})
	_, out, _ := do("validate", "--format", "json")
	if !strings.Contains(out, `"DOC206"`) || strings.Contains(out, `"unsafe"`) {
		t.Fatalf("a self reference in the text has no fix:\n%s", out)
	}
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","links":"x","body":"[[b]]"}`,
		"docs/src/b.json":     `{"title":"B"}`,
	})
	_, out, _ = do("validate", "--format", "json")
	if strings.Contains(out, `"safe"`) {
		t.Fatalf("links of a wrong type cannot be fixed:\n%s", out)
	}
	// a visible document must not get a link to a hidden one
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","notes":["x"],"body":"[[h]]"}`,
		"docs/src/h.json":     `{"title":"H","hidden":true,"links":["index"]}`,
	})
	_, out, _ = do("validate", "--format", "json")
	if strings.Contains(out, `"safe"`) {
		t.Fatalf("no fix may create a link to a hidden document:\n%s", out)
	}
}

func TestBrokenFileStaysANode(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json":  `{"title":"I","links":["broken"]}`,
		"docs/src/broken.json": `{`,
		"docs/src/x.json":      `{"title":"X","parent":"broken"}`,
	})
	_, out, _ := do("validate", "--format", "json")
	if strings.Contains(out, "DOC203") || !strings.Contains(out, "DOC101") {
		t.Fatalf("x is reachable through the broken node:\n%s", out)
	}
}

func TestSimilarTitlesReportedPerPair(t *testing.T) {
	project(t, map[string]string{
		"docc.json":           `{"rules":{"DOC306":"warn"}}`,
		"docs/src/index.json": `{"title":"I","links":["a","b","c"]}`,
		"docs/src/a.json":     `{"title":"user login page one"}`,
		"docs/src/b.json":     `{"title":"user login page onf"}`,
		"docs/src/c.json":     `{"title":"user login page ond"}`,
	})
	_, out, _ := do("validate", "--format", "json")
	if strings.Count(out, `"DOC306"`) != 3 {
		t.Fatalf("three pairs, three reports:\n%s", out)
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
		`{"kinds":["a"],"customRules":[{"id":"custom/x","level":"warn","kind":"zz","given":"/a","forbid":"x","message":"m"}]}`: "docc.json:1:",
		`{"rules":{"DOC301":{"words":["бы*стро"]}}}`:                                                                           "docc.json:1:",
	} {
		project(t, map[string]string{"docs/src/index.json": `{"title":"I"}`, "docc.json": in})
		if code, _, errs := do("validate"); code != 2 || !strings.HasPrefix(errs, want) {
			t.Errorf("%s: %d %q", in, code, errs)
		}
	}
}
