package main

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestRulesList(t *testing.T) {
	rawProject(t, map[string]string{})
	code, out, _ := do("rules", "--format", "json")
	if code != 0 {
		t.Fatal(code)
	}
	var rows []map[string]string
	if err := json.Unmarshal([]byte(out), &rows); err != nil {
		t.Fatal(err)
	}
	if len(rows) != 37 { // 38 codes minus the generic DOC800 row
		t.Fatalf("%d rules", len(rows))
	}
	got := map[string]map[string]string{}
	for _, r := range rows {
		got[r["code"]] = r
	}
	if r := got["DOC301"]; r["level"] != "warn" || r["group"] != "text" || r["rule"] != "text/forbidden-words" || r["description"] == "" {
		t.Errorf("DOC301: %v", r)
	}
	if got["DOC352"]["level"] != "warn" || got["DOC101"]["level"] != "error" || got["DOC302"] != nil || strings.Contains(out, "fixable") {
		t.Error("levels wrong, a retired rule or the fixable column is listed")
	}
	_, text, _ := do("rules")
	lines := strings.Split(strings.TrimSpace(text), "\n")
	if !strings.HasPrefix(lines[0], "CODE") || len(lines) != 38 || strings.Contains(lines[0], "FIXABLE") {
		t.Fatalf("table:\n%s", text)
	}
	for _, ln := range lines {
		if ln != strings.TrimRight(ln, " ") {
			t.Fatalf("trailing spaces: %q", ln)
		}
	}
}

func TestRulesEffectiveLevels(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{
		  "rules": { "DOC313": "off", "text/forbidden-words": "error" },
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
			m[r["rule"]] = r["level"]
		}
		return m
	}
	base := level()
	if base["text/repeated-word"] != "off" || base["text/forbidden-words"] != "error" || base["text/placeholder"] != "warn" || base["custom/x"] != "warn" {
		t.Errorf("config levels: %v", base)
	}
	if code, _, _ := do("rules", "--file", "docs/src/index.json"); code != 2 {
		t.Error("--file is gone")
	}
}

func TestExplain(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{"customRules":[{"id":"custom/x","level":"warn","message":"No x","hint":"Use y","given":"/a","forbid":"x+"}]}`,
	})
	code, out, _ := do("explain", "DOC301")
	if code != 0 || !strings.HasPrefix(out, "DOC301 text/forbidden-words\nGroup: text\nLevel: warn\n") ||
		!strings.Contains(out, "Violation:") || !strings.Contains(out, "Fixed:") || strings.Contains(out, "Automatic fix") {
		t.Fatalf("%d\n%s", code, out)
	}
	if _, byName, _ := do("explain", "text/forbidden-words"); byName != out {
		t.Error("explain by name differs from explain by code")
	}
	code, out, _ = do("explain", "DOC214", "--format", "json")
	var e map[string]string
	if err := json.Unmarshal([]byte(out), &e); err != nil || code != 0 || e["code"] != "DOC214" || strings.Contains(out, "fix") {
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

func TestTraceNeedsScan(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I"}`,
		"tests/a_test.txt":    "// docc: ghost\n",
	})
	if code, out, _ := do("validate"); code != 0 {
		t.Fatalf("without trace.scan DOC401 must not run: %d %s", code, out)
	}
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I"}`,
		"tests/a_test.txt":    "// docc: ghost\n",
		"docc.json":           `{"trace":{"scan":["tests/**"]}}`,
	})
	if code, out, _ := do("validate"); code != 1 || !strings.Contains(out, "tests/a_test.txt:1:4: error DOC401") {
		t.Fatalf("%d %s", code, out)
	}
}

func TestExplainCustomRequire(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{"customRules":[{"id":"custom/needs-y","level":"error","message":"Need y","given":"/a","require":"y+"}]}`,
	})
	code, out, errs := do("explain", "custom/needs-y")
	if code != 0 || !strings.Contains(out, "does not match y+") {
		t.Fatalf("%d %s %s", code, out, errs)
	}
}

func TestExplainShowsEffectiveLevelAndConfigure(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{"rules":{"DOC313":"error"},"customRules":[{"id":"custom/x","level":"warn","message":"No x","hint":"Use y","given":"/a","forbid":"x+"}]}`,
	})
	_, out, _ := do("explain", "DOC313")
	if !strings.Contains(out, "Level: error (default: warn)") || !strings.Contains(out, `"rules": { "DOC313": "off" }`) || !strings.Contains(out, `"docIgnore"`) {
		t.Fatalf("%s", out)
	}
	if _, out, _ = do("explain", "DOC101"); !strings.Contains(out, "cannot be disabled") {
		t.Fatalf("%s", out)
	}
	_, out, _ = do("explain", "custom/x")
	if !strings.Contains(out, "No x") || !strings.Contains(out, "x+") || !strings.Contains(out, "Use y") {
		t.Fatalf("custom: %s", out)
	}
	_, out, _ = do("rules")
	if strings.Index(out, "custom/x") > strings.Index(out, "DOC901") || !strings.Contains(out, "DESCRIPTION") {
		t.Fatalf("custom rules go where DOC800 is:\n%s", out)
	}
}
