package schema

import (
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/santiment/docc/internal/loader"
)

func parse(t *testing.T, s string) *loader.Result {
	t.Helper()
	r := loader.Parse([]byte(s))
	if r.Err != nil {
		t.Fatal(r.Err)
	}
	return r
}

func load(t *testing.T, files map[string]string) (*Set, error) {
	t.Helper()
	dir := filepath.Join(t.TempDir(), "schemas")
	for name, content := range files {
		p := filepath.Join(dir, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	return Load(dir, "docs/schemas")
}

func TestBuiltin(t *testing.T) {
	s, err := load(t, nil)
	if err != nil {
		t.Fatal(err)
	}
	ok := parse(t, `{"title":"A","kind":"note","links":["a","b"],"body":["x","y"],"hidden":true,"reviewed":{"b":"0a1b2c3d"},"free":{"any":1}}`)
	if vs := s.CheckBuiltin(ok.Root); len(vs) != 0 {
		t.Fatalf("unexpected: %+v", vs)
	}

	cases := map[string]struct {
		in   string
		want string // substring of the message
		at   string // expected line:col of the start
	}{
		"dup link":    {"{\n\"title\":\"A\",\n\"links\":[\"b\",\"c\",\"b\"]}", "items at 0 and 2", "3:18"},
		"dup tag":     {`{"title":"A","tags":["x","x"]}`, "equal", "1:26"},
		"title type":  {`{"title":5}`, "/title", "1:10"},
		"kind chars":  {`{"title":"A","kind":"Bad Kind"}`, "/kind", "1:21"},
		"body shape":  {`{"title":"A","body":5}`, "does not match any allowed shape", "1:21"},
		"parent type": {`{"title":"A","parent":["x"]}`, "/parent", "1:23"},
		"hidden type": {`{"title":"A","hidden":"yes"}`, "/hidden", "1:23"},
		"reviewed":    {`{"title":"A","reviewed":{"b":"xyz"}}`, "/reviewed/b", "1:30"},
	}
	for name, c := range cases {
		vs := s.CheckBuiltin(parse(t, c.in).Root)
		if len(vs) != 1 {
			t.Errorf("%s: got %d violations: %+v", name, len(vs), vs)
			continue
		}
		if !strings.Contains(vs[0].Message, c.want) {
			t.Errorf("%s: message %q lacks %q", name, vs[0].Message, c.want)
		}
		r := parse(t, c.in)
		l, col := r.Src.Position(vs[0].Start)
		if got := strconv.Itoa(l) + ":" + strconv.Itoa(col); got != c.at {
			t.Errorf("%s: position %s want %s", name, got, c.at)
		}
	}
}

func TestBlankTitleLeftToDOC103(t *testing.T) {
	s, _ := load(t, nil)
	for _, in := range []string{`{}`, `{"title":""}`, `{"title":"  "}`} {
		if vs := s.CheckBuiltin(parse(t, in).Root); len(vs) != 0 {
			t.Errorf("%s: %+v", in, vs)
		}
	}
}

func TestKindSchema(t *testing.T) {
	s, err := load(t, map[string]string{
		"requirement.json": `{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object",
		  "required":["statement"],"properties":{"status":{"enum":["draft","accepted"]},"statement":{"$ref":"common.json#/$defs/text"}},
		  "additionalProperties":false,"patternProperties":{"^(title|kind|parent|links|tags|body)$":true}}`,
		"common.json": `{"$defs":{"text":{"type":"string","minLength":3}}}`,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !s.HasKind("requirement") || s.HasKind("common") == false {
		t.Fatal("kinds loaded")
	}
	doc := parse(t, "{\n\"title\":\"A\",\"kind\":\"requirement\",\n\"status\":\"wip\",\"extra\":1}")
	vs := s.CheckKind("requirement", doc.Root)
	var msgs []string
	for _, v := range vs {
		msgs = append(msgs, v.Message)
	}
	joined := strings.Join(msgs, "\n")
	for _, w := range []string{"missing property 'statement'", "/status", "property 'extra' is not allowed"} {
		if !strings.Contains(joined, w) {
			t.Errorf("missing %q in:\n%s", w, joined)
		}
	}
	// additionalProperties points at the key
	for _, v := range vs {
		if strings.Contains(v.Message, "'extra'") {
			if l, c := doc.Src.Position(v.Start); l != 3 || c != 16 {
				t.Errorf("extra key position %d:%d", l, c)
			}
		}
	}
	ok := parse(t, `{"title":"A","kind":"requirement","status":"draft","statement":"abcd","notes":["n"],"hidden":true}`)
	if vs := s.CheckKind("requirement", ok.Root); len(vs) != 0 {
		t.Errorf("service fields must be ignored: %+v", vs)
	}
}

func TestBadSchemaFiles(t *testing.T) {
	for name, files := range map[string]map[string]string{
		"invalid json": {"a.json": `{`},
		"bad schema":   {"a.json": `{"type":"nonsense"}`},
		"remote ref":   {"a.json": `{"$ref":"https://example.com/x.json"}`},
		"outside ref":  {"a.json": `{"$ref":"../x.json"}`},
		"missing ref":  {"a.json": `{"$ref":"nope.json"}`},
	} {
		if _, err := load(t, files); err == nil {
			t.Errorf("%s: expected error", name)
		}
	}
}

func TestResolvePointer(t *testing.T) {
	r := parse(t, `{"a":[{"b/c":"x"}],"d":1}`)
	if n := ResolvePointer(r.Root, "/a/0/b~1c"); n == nil || n.Str != "x" {
		t.Error("nested")
	}
	if ResolvePointer(r.Root, "/a/1") != nil || ResolvePointer(r.Root, "a") != nil {
		t.Error("should be nil")
	}
	if ResolvePointer(r.Root, "") != r.Root {
		t.Error("root")
	}
}

func compileBytes(t *testing.T, b []byte) interface{ Validate(any) error } {
	t.Helper()
	r := parse2(t, b)
	l := memLoader{"docc://t/x.json": loader.Value(r.Root)}
	sc, err := newCompiler(l).Compile("docc://t/x.json")
	if err != nil {
		t.Fatal(err)
	}
	return sc
}

func parse2(t *testing.T, b []byte) *loader.Result {
	t.Helper()
	r := loader.Parse(b)
	if r.Err != nil {
		t.Fatal(r.Err)
	}
	return r
}

const specConfig = `{
  "src": "docs/src", "out": "docs/out", "schemas": "docs/schemas", "outFull": "docs/out-full",
  "roots": ["index"], "kinds": ["note", "requirement"], "tags": ["draft"], "recommended": true,
  "rules": {
    "text/forbidden-words": { "level": "warn", "words": ["a"], "allow": [] },
    "style/sentence-length": { "level": "warn", "max": 30 },
    "text/max-length": { "level": "warn", "max": 2000 },
    "markdown/raw-html": "off", "nursery": "warn", "custom/x": "off", "DOC800": "off"
  },
  "overrides": [{ "include": ["drafts/**"], "rules": { "text/placeholder": "off" } }],
  "customRules": [{ "id": "custom/no-vague", "level": "warn", "kind": "requirement", "given": "/statement",
    "forbid": "x", "message": "m", "hint": "h" }],
  "trace": { "scan": ["tests/**"] }, "maxWarnings": 3, "$schema": "x"
}`

func TestConfigSchema(t *testing.T) {
	sc := compileBytes(t, Config())
	ok := parse2(t, []byte(specConfig))
	if err := sc.Validate(loader.Value(ok.Root)); err != nil {
		t.Fatalf("spec example must validate: %v", err)
	}
	for name, bad := range map[string]string{
		"unknown key":   `{"foo":1}`,
		"bad level":     `{"rules":{"DOC301":"loud"}}`,
		"unknown rule":  `{"rules":{"nonsense":"off"}}`,
		"empty roots":   `{"roots":[]}`,
		"both rx":       `{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a","require":"b"}]}`,
		"bad custom id": `{"customRules":[{"id":"a","level":"warn","message":"m","given":"/x","forbid":"a"}]}`,
	} {
		r := parse2(t, []byte(bad))
		if err := sc.Validate(loader.Value(r.Root)); err == nil {
			t.Errorf("%s: should fail", name)
		}
	}
}

func TestDocumentSchemaIsValid(t *testing.T) {
	sc := compileBytes(t, Document())
	r := parse2(t, []byte(`{"title":"A","docc-ignore":[{"rule":"DOC301","reason":"x"}]}`))
	if err := sc.Validate(loader.Value(r.Root)); err != nil {
		t.Fatal(err)
	}
}

func TestFormatIsAnAssertion(t *testing.T) {
	s, err := load(t, map[string]string{"event.json": `{"properties":{"due":{"type":"string","format":"date"}}}`})
	if err != nil {
		t.Fatal(err)
	}
	if vs := s.CheckKind("event", parse(t, `{"title":"A","due":"31.12.2026"}`).Root); len(vs) != 1 {
		t.Errorf("bad date: %+v", vs)
	}
	if vs := s.CheckKind("event", parse(t, `{"title":"A","due":"2026-12-31"}`).Root); len(vs) != 0 {
		t.Errorf("good date: %+v", vs)
	}
}
