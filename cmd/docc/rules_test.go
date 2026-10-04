package main

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestRulesList(t *testing.T) {
	project(t, map[string]string{})
	code, out, _ := do("rules", "--format", "json")
	if code != 0 {
		t.Fatal(code)
	}
	var rows []map[string]string
	if err := json.Unmarshal([]byte(out), &rows); err != nil {
		t.Fatal(err)
	}
	if len(rows) != 39 { // 40 codes minus the generic DOC800 row
		t.Fatalf("%d rules", len(rows))
	}
	got := map[string]map[string]string{}
	for _, r := range rows {
		got[r["code"]] = r
	}
	if r := got["DOC301"]; r["level"] != "warn" || r["group"] != "text" || r["name"] != "text/forbidden-words" || r["fix"] != "none" {
		t.Errorf("DOC301: %v", r)
	}
	if got["DOC208"]["fix"] != "safe" || got["DOC313"]["fix"] != "unsafe" || got["DOC302"]["level"] != "off" || got["DOC101"]["level"] != "error" {
		t.Error("levels or fix kinds wrong")
	}
	_, text, _ := do("rules")
	lines := strings.Split(strings.TrimSpace(text), "\n")
	if !strings.HasPrefix(lines[0], "CODE") || len(lines) != 40 {
		t.Fatalf("table:\n%s", text)
	}
	for _, ln := range lines {
		if ln != strings.TrimRight(ln, " ") {
			t.Fatalf("trailing spaces: %q", ln)
		}
	}
}

func TestRulesEffectiveLevelsAndFile(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{
		  "rules": { "DOC302": "warn", "text/forbidden-words": "error" },
		  "overrides": [{ "include": ["drafts/**"], "rules": { "text/placeholder": "off", "DOC302": "off" } }],
		  "customRules": [{ "id": "custom/x", "level": "warn", "message": "m", "given": "/a", "forbid": "q" }]
		}`,
	})
	level := func(args ...string) map[string]string {
		_, out, errs := do(append([]string{"rules", "--format", "json"}, args...)...)
		var rows []map[string]string
		if err := json.Unmarshal([]byte(out), &rows); err != nil {
			t.Fatalf("%v: %v %s", args, err, errs)
		}
		m := map[string]string{}
		for _, r := range rows {
			m[r["name"]] = r["level"]
		}
		return m
	}
	base := level()
	if base["text/max-length"] != "warn" || base["text/forbidden-words"] != "error" || base["text/placeholder"] != "warn" || base["custom/x"] != "warn" {
		t.Errorf("config levels: %v", base)
	}
	for _, f := range []string{"drafts/a.json", "docs/src/drafts/deep/a.json"} {
		d := level("--file", f)
		if d["text/placeholder"] != "off" || d["text/max-length"] != "off" || d["text/forbidden-words"] != "error" {
			t.Errorf("--file %s: %v", f, d)
		}
	}
	if o := level("--file", "docs/src/other.json"); o["text/placeholder"] != "warn" {
		t.Errorf("override leaked: %v", o)
	}
}

func TestOverridesAffectValidate(t *testing.T) {
	project(t, map[string]string{
		"docc.json":                         `{"overrides":[{"include":["drafts/**"],"rules":{"text/placeholder":"off"}}]}`,
		"docs/src/index.json":               `{"title":"I","links":["w","drafts-index"],"body":"TODO"}`,
		"docs/src/w.json":                   `{"title":"W","parent":"index"}`,
		"docs/src/drafts/drafts-index.json": `{"title":"D","parent":"index","body":"TODO here"}`,
	})
	_, out, _ := do("validate", "--format", "json")
	if strings.Count(out, `"DOC303"`) != 1 || !strings.Contains(out, "docs/src/index.json") {
		t.Fatalf("override must silence only drafts/**:\n%s", out)
	}
}

func TestExplain(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{"customRules":[{"id":"custom/x","level":"warn","message":"No x","hint":"Use y","given":"/a","forbid":"x+"}]}`,
	})
	code, out, _ := do("explain", "DOC301")
	if code != 0 || !strings.HasPrefix(out, "DOC301 text/forbidden-words\nGroup: text\nDefault level: warn\n") ||
		!strings.Contains(out, "Violation:") || !strings.Contains(out, "Fixed:") || !strings.Contains(out, "Automatic fix: none") {
		t.Fatalf("%d\n%s", code, out)
	}
	if _, byName, _ := do("explain", "text/forbidden-words"); byName != out {
		t.Error("explain by name differs from explain by code")
	}
	code, out, _ = do("explain", "DOC208", "--format", "json")
	var e map[string]string
	if err := json.Unmarshal([]byte(out), &e); err != nil || code != 0 || e["fix"] != "safe" || e["code"] != "DOC208" || e["fixDescription"] == "" {
		t.Fatalf("%v %v", err, e)
	}
	if code, out, _ := do("explain", "custom/x"); code != 0 || !strings.Contains(out, "DOC800 custom/x") || !strings.Contains(out, "x+") {
		t.Fatalf("custom: %d %s", code, out)
	}
	for _, bad := range [][]string{{"explain", "DOC999"}, {"explain"}, {"explain", "DOC301", "DOC302"}, {"explain", "custom/none"}} {
		if code, _, errs := do(bad...); code != 2 || errs == "" {
			t.Errorf("%v: %d %q", bad, code, errs)
		}
	}
}

func TestWordsFiles(t *testing.T) {
	project(t, map[string]string{
		"docc.json":           `{"rules":{"DOC301":{"wordsFile":"lists/bad.txt","allowFile":"lists/ok.txt"}}}`,
		"lists/bad.txt":       "# comment\n\nсрочн*\n",
		"lists/ok.txt":        "срочная доставка\n",
		"docs/src/index.json": `{"title":"I","body":["Нужно срочно.","срочная доставка","Это fast."]}`,
	})
	_, out, _ := do("validate", "--format", "json")
	if strings.Count(out, `"DOC301"`) != 2 || strings.Contains(out, "доставка") {
		t.Fatalf("wordsFile adds to the built-ins, allowFile excludes:\n%s", out)
	}
	project(t, map[string]string{
		"docc.json":           `{"rules":{"DOC301":{"wordsFile":"missing.txt"}}}`,
		"docs/src/index.json": `{"title":"I"}`,
	})
	if code, _, errs := do("validate"); code != 2 || !strings.Contains(errs, "config error") {
		t.Fatalf("%d %s", code, errs)
	}
}

func TestTraceNeedsScan(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I"}`,
		"tests/a_test.txt":    "// @covers ghost\n",
	})
	if code, out, _ := do("validate"); code != 0 {
		t.Fatalf("without trace.scan DOC401 must not run: %d %s", code, out)
	}
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I"}`,
		"tests/a_test.txt":    "// @covers ghost\n",
		"docc.json":           `{"trace":{"scan":["tests/**"]}}`,
	})
	if code, out, _ := do("validate"); code != 1 || !strings.Contains(out, "tests/a_test.txt:1:4: error DOC401") {
		t.Fatalf("%d %s", code, out)
	}
}
