package main

import (
	"bytes"
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
	if code != 2 || out != "" || !strings.HasPrefix(errs, "docc.json:2:") || !strings.Contains(errs, "config error") {
		t.Fatalf("%d %q %q", code, out, errs)
	}
}

func TestCleanProject(t *testing.T) {
	project(t, map[string]string{"docs/src/index.json": `{"title":"I"}`})
	code, out, _ := do("validate")
	if code != 0 || out != "0 errors, 0 warnings in 0 files\n" {
		t.Fatalf("%d %q", code, out)
	}
	code, out, _ = do("validate", "--format", "json")
	if code != 0 || out != "[]\n" {
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
		`docs/src/o0.json:1:1: error DOC203 document "o0" is not reachable from the roots [index]`,
		"  hint: Link it from a reachable document (links or [[id]] in the text) or set its parent.",
		`docs/src/o1.json:1:1: error DOC203 document "o1" is not reachable from the roots [index]`,
		"  hint: Link it from a reachable document (links or [[id]] in the text) or set its parent.",
		"показано 2 из 5",
		"5 errors, 0 warnings in 5 files",
	}
	if strings.Join(lines, "\n") != strings.Join(want, "\n") {
		t.Fatalf("got:\n%s", out)
	}
	_, out, _ = do("validate", "--format", "json", "--max-diagnostics", "2")
	if strings.Count(out, `"code"`) != 5 {
		t.Fatal("json output must not be limited")
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
