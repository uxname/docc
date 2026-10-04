package main

import (
	"strings"
	"testing"
)

const idx = `{"title":"I","links":["a","b","c"]}`

func fixed(t *testing.T, a string, args ...string) string {
	t.Helper()
	project(t, map[string]string{
		"docs/src/index.json": idx,
		"docs/src/a.json":     a,
		"docs/src/b.json":     `{"title":"B","parent":"index"}`,
		"docs/src/c.json":     `{"title":"C","parent":"index"}`,
	})
	if code, _, errs := do(append([]string{"validate", "--fix"}, args...)...); code == 2 {
		t.Fatalf("usage error: %s", errs)
	}
	got := readFile(t, "docs/src/a.json")
	// idempotent: a second run changes nothing
	do(append([]string{"validate", "--fix"}, args...)...)
	if again := readFile(t, "docs/src/a.json"); again != got {
		t.Fatalf("not idempotent:\n%s\n---\n%s", got, again)
	}
	return got
}

func TestFixAddsLinks(t *testing.T) {
	cases := map[string][2]string{
		"append to links":    {`{"title":"A","parent":"index","links":["c"],"body":"See [[b]]."}`, `{"title":"A","parent":"index","links":["c", "b"],"body":"See [[b]]."}`},
		"empty links":        {`{"title":"A","parent":"index","links":[],"body":"See [[b]]."}`, `{"title":"A","parent":"index","links":["b"],"body":"See [[b]]."}`},
		"one line, no links": {`{"title":"A","parent":"index","body":"See [[b]]."}`, `{"title":"A","parent":"index","body":"See [[b]].", "links": ["b"]}`},
		"indent copied": {"{\n    \"title\": \"A\",\n    \"parent\": \"index\",\n    \"body\": \"See [[b]].\"\n}\n",
			"{\n    \"title\": \"A\",\n    \"parent\": \"index\",\n    \"body\": \"See [[b]].\",\n    \"links\": [\"b\"]\n}\n"},
		"two ids need two passes": {`{"title":"A","parent":"index","body":"[[b]] and [[c]]"}`, `{"title":"A","parent":"index","body":"[[b]] and [[c]]", "links": ["b", "c"]}`},
	}
	for name, c := range cases {
		if got := fixed(t, c[0]); got != c[1] {
			t.Errorf("%s:\n got %s\nwant %s", name, got, c[1])
		}
	}
}

func TestFixKeepsBOMAndCRLF(t *testing.T) {
	in := "\xEF\xBB\xBF{\r\n  \"title\": \"A\",\r\n  \"parent\": \"index\",\r\n  \"body\": \"See [[b]].\"\r\n}\r\n"
	want := "\xEF\xBB\xBF{\r\n  \"title\": \"A\",\r\n  \"parent\": \"index\",\r\n  \"body\": \"See [[b]].\",\n  \"links\": [\"b\"]\r\n}\r\n"
	if got := fixed(t, in); got != want {
		t.Errorf("got %q\nwant %q", got, want)
	}
}

func TestUnsafeFixes(t *testing.T) {
	cases := map[string][2]string{
		"self link, last":   {`{"title":"A","parent":"index","links":["b","a"]}`, `{"title":"A","parent":"index","links":["b"]}`},
		"self link, first":  {`{"title":"A","parent":"index","links":["a","b"]}`, `{"title":"A","parent":"index","links":["b"]}`},
		"self link, only":   {`{"title":"A","parent":"index","links":["a"]}`, `{"title":"A","parent":"index","links":[]}`},
		"self parent":       {`{"title":"A","parent":"a","body":"x"}`, `{"title":"A","body":"x"}`},
		"self parent, last": {`{"title":"A","body":"x","parent":"a"}`, `{"title":"A","body":"x"}`},
		"repeated word":     {`{"title":"A","parent":"index","body":"The the system works. Done done."}`, `{"title":"A","parent":"index","body":"The system works. Done."}`},
	}
	for name, c := range cases {
		if got := fixed(t, c[0], "--unsafe"); got != c[1] {
			t.Errorf("%s:\n got %s\nwant %s", name, got, c[1])
		}
		// without --unsafe nothing changes
		if got := fixed(t, c[0]); got != c[0] {
			t.Errorf("%s: --fix alone must not apply unsafe fixes, got %s", name, got)
		}
	}
}

func TestFixRespectsIgnoreAndEscapes(t *testing.T) {
	in := `{"title":"A","parent":"index","body":"in in","docIgnore":[{"rule":"DOC313","reason":"quoted"}]}`
	if got := fixed(t, in, "--unsafe"); got != in {
		t.Errorf("suppressed diagnostics must not be fixed: %s", got)
	}
	bs := "\\"
	in = `{"title":"A","parent":"index","body":"` + bs + `u0430 ` + bs + `u0430 y"}`
	want := `{"title":"A","parent":"index","body":"` + bs + `u0430 y"}`
	if got := fixed(t, in, "--unsafe"); got != want {
		t.Errorf("escaped word:\n got %s\nwant %s", got, want)
	}
}

func TestFixOutputAndExitCode(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","links":["a"],"body":"[[a]] [[a]]"}`,
		"docs/src/a.json":     `{"title":"A","parent":"index","body":"The the end"}`,
	})
	code, out, _ := do("validate", "--fix")
	if code != 0 || !strings.Contains(out, "0 errors, 1 warning") {
		t.Fatalf("%d %s", code, out)
	}
	code, out, _ = do("validate", "--fix", "--unsafe")
	if code != 0 || !strings.Contains(out, "applied 1 fixes in 1 files") || !strings.Contains(out, "0 errors, 0 warnings") {
		t.Fatalf("%d %s", code, out)
	}
}
