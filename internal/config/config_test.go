package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/santiment/docc/internal/diag"
)

func mustParse(t *testing.T, s string) *Config {
	t.Helper()
	c, err := Parse([]byte(s), t.TempDir(), "docc.json")
	if err != nil {
		t.Fatalf("%s: %v", s, err)
	}
	return c
}

func TestDefaults(t *testing.T) {
	c := mustParse(t, `{}`)
	r := c.For("a.json")
	if r.Rule("DOC301").Level != diag.Warn || r.Rule("DOC302").Level != diag.Off || r.Rule("DOC101").Level != diag.Error {
		t.Fatal("preset levels wrong")
	}
	if c.Src != "docs/src" || c.Roots[0] != "index" {
		t.Fatal("defaults")
	}
}

func TestPrecedence(t *testing.T) {
	c := mustParse(t, `{"rules":{"nursery":"warn","style":"error","DOC351":"off","text/max-length":{"level":"error","max":10}}}`)
	r := c.For("a.json")
	if r.Rule("DOC305").Level != diag.Warn { // nursery
		t.Error("nursery")
	}
	if r.Rule("DOC352").Level != diag.Error { // group beats nursery
		t.Error("group")
	}
	if r.Rule("DOC351").Level != diag.Off { // rule beats group
		t.Error("rule")
	}
	if s := r.Rule("DOC302"); s.Level != diag.Error || s.Params.Max != 10 {
		t.Error("params", s)
	}
}

func TestRecommendedFalse(t *testing.T) {
	r := mustParse(t, `{"recommended":false}`).For("a.json")
	if r.Rule("DOC301").Level != diag.Off || r.Rule("DOC201").Level != diag.Error {
		t.Fatal("recommended=false")
	}
}

func TestGroupDoesNotTouchCore(t *testing.T) {
	r := mustParse(t, `{"rules":{"syntax":"off","graph":"off"}}`).For("a.json")
	if r.Rule("DOC101").Level != diag.Error || r.Rule("DOC201").Level != diag.Error {
		t.Fatal("core changed by group")
	}
	if r.Rule("DOC206").Level != diag.Off {
		t.Fatal("non-core graph rule should be off")
	}
}

func TestOverrides(t *testing.T) {
	c := mustParse(t, `{"rules":{"DOC301":{"level":"warn","words":["a"]}},
	  "overrides":[{"include":["drafts/**"],"rules":{"text/placeholder":"off"}},
	               {"include":["drafts/x/**"],"rules":{"DOC301":"error"}}]}`)
	if c.For("drafts/a.json").Rule("DOC303").Level != diag.Off {
		t.Error("override off")
	}
	if c.For("other.json").Rule("DOC303").Level != diag.Warn {
		t.Error("override leaked")
	}
	s := c.For("drafts/x/a.json").Rule("DOC301")
	if s.Level != diag.Error || len(s.Params.Words) != 0 { // later entry replaces the whole setting
		t.Errorf("replace whole: %+v", s)
	}
}

func TestErrors(t *testing.T) {
	for name, in := range map[string]string{
		"unknown key":      `{"foo":1}`,
		"bad type":         `{"src":1}`,
		"abs path":         `{"src":"/etc"}`,
		"empty roots":      `{"roots":[]}`,
		"core by rule":     `{"rules":{"DOC101":"off"}}`,
		"both code+name":   `{"rules":{"DOC301":"off","text/forbidden-words":"warn"}}`,
		"unknown rule":     `{"rules":{"nope":"off"}}`,
		"bad level":        `{"rules":{"DOC301":"loud"}}`,
		"bad param":        `{"rules":{"DOC301":{"max":1}}}`,
		"group object":     `{"rules":{"text":{"level":"off"}}}`,
		"dup key":          `{"src":"a","src":"b"}`,
		"bad regexp":       `{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"("}]}`,
		"forbid+require":   `{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a","require":"b"}]}`,
		"dup custom":       `{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a"},{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a"}]}`,
		"bad custom id":    `{"customRules":[{"id":"x/a","level":"warn","message":"m","given":"/x","forbid":"a"}]}`,
		"custom kind":      `{"kinds":["note"],"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a","kind":"memo"}]}`,
		"unknown custom":   `{"rules":{"custom/zzz":"off"}}`,
		"bad max warnings": `{"maxWarnings":-1}`,
		"missing words":    `{"rules":{"DOC301":{"wordsFile":"nope.txt"}}}`,
	} {
		_, err := Parse([]byte(in), t.TempDir(), "docc.json")
		if err == nil {
			t.Errorf("%s: expected error", name)
		} else if !strings.Contains(err.Error(), "config error") {
			t.Errorf("%s: %v", name, err)
		}
	}
}

func TestErrorPosition(t *testing.T) {
	_, err := Parse([]byte("{\n  \"src\": 1\n}"), t.TempDir(), "docc.json")
	if err == nil || err.Error() != "docc.json:2:10: config error: expected a string" {
		t.Fatalf("%v", err)
	}
}

func TestCustomRules(t *testing.T) {
	c := mustParse(t, `{"customRules":[{"id":"custom/a","level":"error","message":"m","given":"/x","forbid":"q"}],
	  "rules":{"custom/a":"warn"}}`)
	if c.For("a.json").Custom("custom/a").Level != diag.Warn {
		t.Error("custom level override")
	}
	c = mustParse(t, `{"customRules":[{"id":"custom/a","level":"error","message":"m","given":"/x","forbid":"q"}],
	  "rules":{"DOC800":"off"}}`)
	if c.For("a.json").Custom("custom/a").Level != diag.Off {
		t.Error("DOC800 switches all custom rules")
	}
}

func TestWordsFile(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "w.txt"), []byte("# c\n\nбыстр*\n и т.д. \n"), 0o644); err != nil {
		t.Fatal(err)
	}
	c, err := Parse([]byte(`{"rules":{"DOC301":{"words":["x"],"wordsFile":"w.txt"}}}`), dir, "docc.json")
	if err != nil {
		t.Fatal(err)
	}
	got := c.For("a.json").Rule("DOC301").Params.Words
	if strings.Join(got, "|") != "x|быстр*|и т.д." {
		t.Fatalf("%q", got)
	}
}
