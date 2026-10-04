package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// project creates a temp project from path -> content and chdirs into it.
func project(t *testing.T, files map[string]string) string {
	t.Helper()
	dir := t.TempDir()
	for name, content := range files {
		p := filepath.Join(dir, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	t.Chdir(dir)
	return dir
}

func do(args ...string) (int, string, string) {
	var out, errb bytes.Buffer
	code := run(args, &out, &errb)
	return code, out.String(), errb.String()
}

// diagnosticsOf returns the "diagnostics" array of a JSON report.
func diagnosticsOf(out string) []byte {
	var rep struct{ Diagnostics json.RawMessage }
	if err := json.Unmarshal([]byte(out), &rep); err != nil {
		return []byte(out)
	}
	return rep.Diagnostics
}

func TestMissingSrcIsUsageError(t *testing.T) {
	project(t, map[string]string{})
	code, out, errs := do("validate")
	if code != 2 || out != "" || !strings.Contains(errs, "docc: error:") {
		t.Fatalf("%d %q %q", code, out, errs)
	}
}

func TestEmptySrcGivesDOC205(t *testing.T) {
	project(t, map[string]string{"docs/src/.keep": ""})
	code, out, _ := do("validate", "--format", "json")
	if code != 1 || !strings.Contains(out, `"DOC205"`) {
		t.Fatalf("%d %s", code, out)
	}
}

func TestBadConfigIsExit2(t *testing.T) {
	project(t, map[string]string{"docc.json": "{\n  \"rules\": { \"DOC101\": \"off\" }\n}", "docs/src/index.json": `{"title":"I"}`})
	code, out, errs := do("validate")
	if code != 2 || out != "" || !strings.HasPrefix(errs, "docc: error: docc.json:2:") || !strings.Contains(errs, "config error") {
		t.Fatalf("%d %q %q", code, out, errs)
	}
}

func TestCleanProject(t *testing.T) {
	project(t, map[string]string{"docs/src/index.json": `{"title":"I"}`})
	code, out, _ := do("validate")
	if code != 0 || out != "checked 1 file: 0 errors, 0 warnings\n" {
		t.Fatalf("%d %q", code, out)
	}
	code, out, _ = do("validate", "--format", "json")
	if code != 0 || !strings.Contains(out, `"summary": {
    "checked": 1,
    "errors": 0,
    "warnings": 0,
    "shown": 0,
    "truncated": false
  }`) || !strings.Contains(out, `"diagnostics": []`) {
		t.Fatalf("%d %q", code, out)
	}
}

func TestTextFormatAndLimit(t *testing.T) {
	files := map[string]string{"docs/src/index.json": `{"title":"I"}`}
	for i := 0; i < 5; i++ {
		files[fmt.Sprintf("docs/src/o%d.json", i)] = `{"title":"O"}`
	}
	project(t, files)
	code, out, _ := do("validate", "--max-diagnostics", "2")
	if code != 1 {
		t.Fatal(code)
	}
	lines := strings.Split(strings.TrimSpace(out), "\n")
	want := []string{
		`docs/src/o0.json:1:1: error DOC203 (graph/orphan) document "o0" is not reachable from the roots [index]`,
		"  hint: Link it from a reachable document (links or [[id]] in the text) or set its parent.",
		`docs/src/o1.json:1:1: error DOC203 (graph/orphan) document "o1" is not reachable from the roots [index]`,
		"  hint: Link it from a reachable document (links or [[id]] in the text) or set its parent.",
		"showing 2 of 5",
		"checked 6 files: 5 errors, 0 warnings",
		"Run docc explain <rule> for details",
	}
	if strings.Join(lines, "\n") != strings.Join(want, "\n") {
		t.Fatalf("got:\n%s", out)
	}
	_, out, _ = do("validate", "--format", "json", "--max-diagnostics", "2")
	if strings.Count(out, `"code"`) != 2 || !strings.Contains(out, `"truncated": true`) || !strings.Contains(out, `"shown": 2`) || !strings.Contains(out, `"errors": 5`) {
		t.Fatalf("json output is limited like text, and says so:\n%s", out)
	}
}

func TestFlagsAfterArguments(t *testing.T) {
	project(t, map[string]string{"docs/src/index.json": `{"title":"I"}`})
	if code, _, errs := do("validate", "--format"); code != 2 || errs == "" {
		t.Fatalf("missing flag value: %d %q", code, errs)
	}
	if code, _, _ := do("validate", "--bogus"); code != 2 {
		t.Fatal("unknown flag")
	}
	if code, _, _ := do("validate", "--unsafe"); code != 2 {
		t.Fatal("--unsafe without --fix")
	}
	if code, _, _ := do("nope"); code != 2 {
		t.Fatal("unknown command")
	}
	if code, out, _ := do("--version"); code != 0 || !strings.HasPrefix(out, "docc ") {
		t.Fatal("version")
	}
}

func TestSchemaCommand(t *testing.T) {
	project(t, map[string]string{})
	code, out, _ := do("schema", "document")
	if code != 0 || !strings.Contains(out, `"title": "docc document"`) {
		t.Fatalf("%d %q", code, out)
	}
	code, out, _ = do("schema", "config")
	if code != 0 || !strings.Contains(out, `"additionalProperties": false`) || !strings.Contains(out, "text/forbidden-words") {
		t.Fatalf("%d %q", code, out)
	}
	if code, _, _ := do("schema"); code != 2 {
		t.Fatal("schema without argument must be a usage error")
	}
	if code, _, _ := do("schema", "other"); code != 2 {
		t.Fatal("unknown schema")
	}
}

func TestBrokenSchemaFilesAreExit2(t *testing.T) {
	for name, files := range map[string]map[string]string{
		"invalid json": {"docs/schemas/a.json": `{`},
		"bad schema":   {"docs/schemas/a.json": `{"type":"nonsense"}`},
		"remote ref":   {"docs/schemas/a.json": `{"$ref":"https://example.com/s.json"}`},
		"outside ref":  {"docs/schemas/a.json": `{"$ref":"../secret.json"}`, "docs/secret.json": `{"type":"string"}`},
	} {
		files["docs/src/index.json"] = `{"title":"I"}`
		project(t, files)
		code, out, errs := do("validate")
		if code != 2 || out != "" || !strings.Contains(errs, "docs/schemas/a.json") {
			t.Errorf("%s: %d %q %q", name, code, out, errs)
		}
	}
}

func TestKindSchemaFileWithLocalRef(t *testing.T) {
	project(t, map[string]string{
		"docs/schemas/note.json":          `{"properties":{"status":{"$ref":"common/status.json"}}}`,
		"docs/schemas/common/status.json": `{"enum":["a","b"]}`,
		"docs/src/index.json":             `{"title":"I","kind":"note","status":"c"}`,
	})
	code, out, _ := do("validate", "--format", "json")
	if code != 1 || !strings.Contains(out, "DOC106") {
		t.Fatalf("%d %s", code, out)
	}
}

func TestMaxWarnings(t *testing.T) {
	files := map[string]string{"docs/src/index.json": `{"title":"I","body":"TODO"}`}
	project(t, files)
	if code, _, _ := do("validate"); code != 0 {
		t.Fatalf("a warning alone is not a failure, got %d", code)
	}
	if code, _, _ := do("validate", "--max-warnings", "0"); code != 1 {
		t.Fatal("--max-warnings 0 must fail on one warning")
	}
	if code, _, _ := do("validate", "--max-warnings", "1"); code != 0 {
		t.Fatal("one warning is within the limit 1")
	}
	files["docc.json"] = `{"maxWarnings":0}`
	project(t, files)
	if code, _, _ := do("validate"); code != 1 {
		t.Fatal("maxWarnings from docc.json")
	}
	if code, _, _ := do("validate", "--max-warnings", "5"); code != 0 {
		t.Fatal("the flag must win over docc.json")
	}
}

func readFile(t *testing.T, name string) string {
	t.Helper()
	b, err := os.ReadFile(name)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func TestBuildCleansOutAndIsIdempotent(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json":    `{"title":"I","links":["a"]}`,
		"docs/src/a.json":        `{"title":"A","parent":"index"}`,
		"docs/out/foreign.txt":   "mine",
		"docs/out/old/deep/x.md": "old",
	})
	if code, _, errs := do("build"); code != 0 {
		t.Fatalf("%d %s", code, errs)
	}
	first := readFile(t, "docs/out/index.md") + readFile(t, "docs/out/_tree.md")
	for _, gone := range []string{"docs/out/foreign.txt", "docs/out/old"} {
		if _, err := os.Stat(gone); err == nil {
			t.Errorf("%s must be removed", gone)
		}
	}
	if code, _, _ := do("check"); code != 0 {
		t.Fatal("check after build must be green")
	}
	do("build")
	if first != readFile(t, "docs/out/index.md")+readFile(t, "docs/out/_tree.md") {
		t.Fatal("two builds differ")
	}
	// stale: change a source without building
	if err := os.WriteFile("docs/src/a.json", []byte(`{"title":"A2","parent":"index"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	code, out, _ := do("check", "--format", "json")
	if code != 1 || !strings.Contains(out, "DOC501") {
		t.Fatalf("%d %s", code, out)
	}
}

func TestBuildStopsOnStructuralErrors(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","links":["nope"]}`,
		"docs/out/keep.md":    "keep",
	})
	code, out, _ := do("build")
	if code != 1 || !strings.Contains(out, "DOC202") {
		t.Fatalf("%d %s", code, out)
	}
	if readFile(t, "docs/out/keep.md") != "keep" {
		t.Fatal("build must not touch out/ when sources have errors")
	}
	if code, _, _ := do("build", "--full"); code != 1 {
		t.Fatal("--full refuses the same errors")
	}
	if _, err := os.Stat("docs/out-full"); err == nil {
		t.Fatal("out-full must not be created")
	}
	// check does not compare out/ when sources are broken
	if code, out, _ := do("check", "--format", "json"); code != 1 || strings.Contains(out, "DOC501") {
		t.Fatalf("check: %d %s", code, out)
	}
}

func TestCheckNormalizesCRLFAndBOM(t *testing.T) {
	project(t, map[string]string{"docs/src/index.json": `{"title":"I"}`})
	do("build")
	for _, f := range []string{"docs/out/index.md", "docs/out/_tree.md"} {
		b := readFile(t, f)
		crlf := "\xEF\xBB\xBF" + strings.ReplaceAll(b, "\n", "\r\n")
		if err := os.WriteFile(f, []byte(crlf), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if code, out, _ := do("check"); code != 0 {
		t.Fatalf("CRLF and BOM must compare equal: %d %s", code, out)
	}
	// and a CRLF source works too
	if err := os.WriteFile("docs/src/index.json", []byte("\xEF\xBB\xBF{\r\n  \"title\": \"I\"\r\n}\r\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if code, out, _ := do("check"); code != 0 {
		t.Fatalf("%d %s", code, out)
	}
}

func TestFullBuildLeavesOutAlone(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","notes":["see [[h]]"]}`,
		"docs/src/h.json":     `{"title":"H","hidden":true,"links":["index"],"notes":["secret"]}`,
	})
	if code, _, errs := do("build"); code != 0 {
		t.Fatalf("%d %s", code, errs)
	}
	before := readFile(t, "docs/out/index.md") + readFile(t, "docs/out/_tree.md")
	if _, err := os.Stat("docs/out/h.md"); err == nil {
		t.Fatal("hidden documents get no page in out/")
	}
	if code, _, errs := do("build", "--full"); code != 0 {
		t.Fatalf("%d %s", code, errs)
	}
	if before != readFile(t, "docs/out/index.md")+readFile(t, "docs/out/_tree.md") {
		t.Fatal("--full must not change out/")
	}
	h := readFile(t, "docs/out-full/h.md")
	if !strings.Contains(h, "> Hidden document.") || !strings.Contains(h, "secret") {
		t.Fatalf("full page of a hidden document:\n%s", h)
	}
	// check ignores out-full: a stray file there is not DOC501
	if err := os.WriteFile("docs/out-full/stray.md", []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if code, out, _ := do("check"); code != 0 {
		t.Fatalf("%d %s", code, out)
	}
	// and --full removes stray files in out-full
	do("build", "--full")
	if _, err := os.Stat("docs/out-full/stray.md"); err == nil {
		t.Fatal("--full must remove extra files in out-full")
	}
}

func TestUnsafeOutDirIsUsageError(t *testing.T) {
	for _, out := range []string{".", "docs", "docs/src", "docs/src/sub"} {
		project(t, map[string]string{
			"docs/src/index.json": `{"title":"I"}`,
			"docc.json":           `{"out":"` + out + `"}`,
		})
		if out == "." {
			// "." is rejected when the config is read
			if code, _, _ := do("build"); code != 2 {
				t.Errorf("out=%q: %d", out, code)
			}
			continue
		}
		code, _, errs := do("build")
		if code != 2 || !strings.Contains(errs, "output directory") {
			t.Errorf("out=%q: %d %q", out, code, errs)
		}
		if _, err := os.Stat("docs/src/index.json"); err != nil {
			t.Fatalf("out=%q destroyed the sources", out)
		}
	}
}

func TestCheckRejectsFix(t *testing.T) {
	project(t, map[string]string{"docs/src/index.json": `{"title":"I"}`})
	if code, _, _ := do("check", "--fix"); code != 2 {
		t.Fatal("--fix with check is a usage error")
	}
}

func TestInit(t *testing.T) {
	dir := t.TempDir()
	t.Chdir(dir)
	if code, out, errs := do("init"); code != 0 || !strings.Contains(out, "created docc.json") {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	if code, out, _ := do("check"); code != 0 {
		t.Fatalf("init output must pass check: %d %s", code, out)
	}
	if err := os.WriteFile("docs/src/index.json", []byte(`{"title":"Mine"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if code, out, _ := do("init"); code != 0 || !strings.Contains(out, "skipped docs/src/index.json") {
		t.Fatalf("%d %s", code, out)
	}
	if !strings.Contains(readFile(t, "docs/src/index.json"), "Mine") {
		t.Fatal("init must not overwrite")
	}
	if !strings.Contains(readFile(t, "docs/out/index.md"), "# Mine") {
		t.Fatal("init always builds")
	}
}

func TestLineBreaksInValuesStayLF(t *testing.T) {
	bs := "\\"
	project(t, map[string]string{"docs/src/index.json": `{"title":"I` + bs + `r` + bs + `nx","body":"a` + bs + `r` + bs + `nb` + bs + `rc"}`})
	if code, _, errs := do("build"); code != 0 {
		t.Fatalf("%d %s", code, errs)
	}
	for _, f := range []string{"docs/out/index.md", "docs/out/_tree.md"} {
		if strings.Contains(readFile(t, f), "\r") {
			t.Errorf("%s contains a carriage return", f)
		}
	}
	if !strings.Contains(readFile(t, "docs/out/index.md"), "# I x\n") {
		t.Error("a line break in the title must become a space in the heading")
	}
	if code, out, _ := do("check"); code != 0 {
		t.Fatalf("check right after build must pass: %s", out)
	}
}

func TestSameOutAndOutFullIsUsageError(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I"}`,
		"docc.json":           `{"out":"docs/out","outFull":"docs/out"}`,
	})
	if code, _, errs := do("build", "--full"); code != 2 || !strings.Contains(errs, "output directory") {
		t.Fatalf("%d %s", code, errs)
	}
}

func TestHelpPerCommand(t *testing.T) {
	project(t, map[string]string{})
	for _, cmd := range []string{"init", "validate", "build", "check", "explain", "rules", "schema"} {
		for _, h := range []string{"--help", "-h"} {
			code, out, errs := do(cmd, h)
			if code != 0 || errs != "" || !strings.HasPrefix(out, "Usage: docc "+cmd) || !strings.Contains(out, "Example") {
				t.Errorf("%s %s: %d %q %q", cmd, h, code, out, errs)
			}
		}
	}
	_, out, _ := do("validate", "--help")
	if !strings.Contains(out, "-unsafe") || !strings.Contains(out, `(default "text")`) {
		t.Errorf("flags with defaults missing:\n%s", out)
	}
	if code, out, _ := do("--format", "json", "--help"); code != 0 || !strings.HasPrefix(out, "docc - ") {
		t.Errorf("%d %q", code, out)
	}
}

func TestGlobalFlagsBeforeCommand(t *testing.T) {
	files := map[string]string{"docs/src/index.json": `{"title":"I"}`}
	for i := 0; i < 3; i++ {
		files[fmt.Sprintf("docs/src/o%d.json", i)] = `{"title":"O"}`
	}
	project(t, files)
	code, out, _ := do("--format", "json", "--max-diagnostics", "1", "validate")
	if code != 1 || !strings.Contains(out, `"truncated": true`) || strings.Count(out, `"code"`) != 1 {
		t.Fatalf("%d %s", code, out)
	}
	if code, out, _ := do("--format", "json", "explain", "DOC301"); code != 0 || !strings.Contains(out, `"fixable": "none"`) {
		t.Fatalf("explain: %d %s", code, out)
	}
	if code, out, _ := do("--format", "json", "rules"); code != 0 || !strings.HasPrefix(out, "[") {
		t.Fatalf("rules: %d %s", code, out)
	}
	if code, out, _ := do("--max-warnings", "3", "schema", "document"); code != 0 || !strings.Contains(out, "docc document") {
		t.Fatalf("schema: %d %s", code, out)
	}
	if code, _, errs := do("--bogus", "validate"); code != 2 || !strings.Contains(errs, "docc: error:") {
		t.Fatalf("%d %s", code, errs)
	}
	if code, _, _ := do("--format", "json"); code != 2 {
		t.Fatal("flags without a command")
	}
	if code, _, errs := do("validate", "--max-diagnostics", "-1"); code != 2 || !strings.Contains(errs, "negative") {
		t.Fatalf("%d %s", code, errs)
	}
}

func TestErrorsAreJSONWithFormatJSON(t *testing.T) {
	project(t, map[string]string{"docc.json": "{\n  \"rules\": { \"DOC101\": \"off\" }\n}", "docs/src/index.json": `{"title":"I"}`})
	for _, args := range [][]string{{"validate", "--format", "json"}, {"--format", "json", "validate"}} {
		code, out, errs := do(args...)
		var e struct {
			Code, Message, Hint, File string
			Line, Column              int
		}
		if err := json.Unmarshal([]byte(out), &e); err != nil || code != 2 || errs != "" || e.Code != "config" || e.File != "docc.json" || e.Line != 2 || e.Column == 0 || e.Message == "" {
			t.Fatalf("%v: %d %v %+v %q", args, code, err, e, errs)
		}
	}
	code, out, _ := do("validate", "--format", "json", "--bogus")
	if code != 2 || !strings.Contains(out, `"code": "usage"`) {
		t.Fatalf("%d %s", code, out)
	}
}

func TestTooManyWarningsExplained(t *testing.T) {
	project(t, map[string]string{"docs/src/index.json": `{"title":"I","body":"TODO"}`})
	code, out, errs := do("validate", "--max-warnings", "0")
	if code != 1 || errs != "too many warnings: 1 > --max-warnings 0\n" || !strings.Contains(out, "checked 1 file: 0 errors, 1 warning\n") {
		t.Fatalf("%d %q %q", code, out, errs)
	}
	code, out, errs = do("validate", "--max-warnings", "0", "--format", "json")
	if code != 1 || errs != "" || !strings.Contains(out, `"failure": "too many warnings: 1 > --max-warnings 0"`) {
		t.Fatalf("%d %q %q", code, out, errs)
	}
}

func TestBuildSkippedAndInitNext(t *testing.T) {
	project(t, map[string]string{"docs/src/index.json": `{"title":"I","links":["nope"]}`})
	code, out, _ := do("build")
	if code != 1 || !strings.Contains(out, "build skipped: 1 errors, docs/out not changed\n") {
		t.Fatalf("%d %s", code, out)
	}
	project(t, map[string]string{})
	code, out, _ = do("init")
	if code != 0 || !strings.Contains(out, "Next:\n  1. edit docs/src/index.json") || !strings.Contains(out, "docc validate --format json") || !strings.Contains(out, "SKILL.md") {
		t.Fatalf("%d %s", code, out)
	}
}
