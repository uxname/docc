package engine

import (
	"strings"
	"testing"

	"github.com/uxname/docc/internal/loader"
)

func fp(t *testing.T, s string) string {
	t.Helper()
	r := loader.Parse([]byte(s))
	if r.Err != nil {
		t.Fatal(r.Err)
	}
	return Fingerprint(r.Root)
}

func canon(t *testing.T, s string) string {
	t.Helper()
	r := loader.Parse([]byte(s))
	var b strings.Builder
	canonical(&b, r.Root, fingerprintSkip)
	return b.String()
}

// Reference values were computed independently with Python
// (json.dumps(sort_keys=True, separators=(',', ':'), ensure_ascii=False), sha256).
func TestFingerprintReference(t *testing.T) {
	cases := map[string]string{
		`{ "title": "Beta", "parent": "index" }`:  "04b48c1c",
		`{ "title": "Gamma", "parent": "index" }`: "1789f10f",
		`{ "title": "Alpha", "parent": "index", "statement": "Первый факт", "n": 3, "items": [1, 2.5, true, null], "notes": ["ignored"] }`: "52a40e88",
	}
	for in, want := range cases {
		if got := fp(t, in); got != want {
			t.Errorf("%s: got %s want %s", in, got, want)
		}
	}
}

func TestFingerprintCanonicalForm(t *testing.T) {
	// notes and docIgnore are dropped at the top level only; keys sorted at every level
	got := canon(t, `{"b":{"z":1,"a":2},"a":[3,"x"],"notes":["n"],"$schema":"s","docIgnore":[],"c":{"notes":1}}`)
	want := `{"$schema":"s","a":[3,"x"],"b":{"a":2,"z":1},"c":{"notes":1}}`
	if got != want {
		t.Errorf("got %s want %s", got, want)
	}
	// numbers keep their source text
	if canon(t, `{"n":2.50}`) != `{"n":2.50}` || fp(t, `{"n":2.50}`) == fp(t, `{"n":2.5}`) {
		t.Error("numbers must stay as written")
	}
	// minimal escaping: control characters as \u00XX, others raw
	in := "{\"s\":\"a\\n\\t\\\"\\\\\\/é\"}"
	if got := canon(t, in); got != "{\"s\":\"a\\u000a\\u0009\\\"\\\\/é\"}" {
		t.Errorf("escaping: %s", got)
	}
	// key order in the file does not matter, whitespace does not matter
	if fp(t, `{"a":1,"b":2}`) != fp(t, "{\n  \"b\": 2,\n  \"a\": 1\n}") {
		t.Error("order or whitespace changed the fingerprint")
	}
}
