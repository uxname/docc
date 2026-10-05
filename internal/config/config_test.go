package config

import (
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
	r := c.Resolve()
	if r.Rule("DOC301").Level != diag.Warn || r.Rule("DOC101").Level != diag.Error {
		t.Fatal("preset levels wrong")
	}
	if c.Src != "docs/src" || c.Roots[0] != "index" {
		t.Fatal("defaults")
	}
}

func TestPrecedence(t *testing.T) {
	c := mustParse(t, `{"rules":{"style":"error","DOC353":"off","text/forbidden-words":{"level":"error","words":["x"]}}}`)
	r := c.Resolve()
	if r.Rule("DOC352").Level != diag.Error { // group beats preset
		t.Error("group")
	}
	if r.Rule("DOC353").Level != diag.Off { // rule beats group
		t.Error("rule")
	}
	if s := r.Rule("DOC301"); s.Level != diag.Error || len(s.Params.Words) != 1 {
		t.Error("params", s)
	}
}

func TestRecommendedFalse(t *testing.T) {
	r := mustParse(t, `{"recommended":false}`).Resolve()
	if r.Rule("DOC301").Level != diag.Off || r.Rule("DOC201").Level != diag.Error {
		t.Fatal("recommended=false")
	}
}

func TestGroupDoesNotTouchCore(t *testing.T) {
	r := mustParse(t, `{"rules":{"syntax":"off","graph":"off"}}`).Resolve()
	if r.Rule("DOC101").Level != diag.Error || r.Rule("DOC201").Level != diag.Error {
		t.Fatal("core changed by group")
	}
	if r.Rule("DOC214").Level != diag.Off {
		t.Fatal("non-core graph rule should be off")
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
		"unknown custom":   `{"rules":{"custom/zzz":"off"}}`,
		"bad max warnings": `{"maxWarnings":-1}`,
		"overrides":        `{"overrides":[]}`,
		"kinds":            `{"kinds":["note"]}`,
		"schema key":       `{"$schema":"x"}`,
		"nursery group":    `{"rules":{"nursery":"warn"}}`,
		"retired rule":     `{"rules":{"DOC302":"off"}}`,
		"words file":       `{"rules":{"DOC301":{"wordsFile":"w.txt"}}}`,
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
	if c.Resolve().Custom("custom/a").Level != diag.Warn {
		t.Error("custom level override")
	}
	c = mustParse(t, `{"customRules":[{"id":"custom/a","level":"error","message":"m","given":"/x","forbid":"q"}],
	  "rules":{"DOC800":"off"}}`)
	if c.Resolve().Custom("custom/a").Level != diag.Off {
		t.Error("DOC800 switches all custom rules")
	}
}
