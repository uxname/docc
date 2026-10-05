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
		  "additionalProperties":false}`,
		"common.json": `{"$defs":{"text":{"type":"string","minLength":3}},"additionalProperties":true}`,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !s.HasKind("requirement") || !s.HasKind("common") {
		t.Fatal("kinds loaded")
	}
	doc := parse(t, "{\n\"title\":\"A\",\"kind\":\"requirement\",\n\"status\":\"wip\",\"extra\":1}")
	vs := s.CheckKind("requirement", doc.Root)
	joined := messages(vs)
	for _, w := range []string{`missing property "statement"`, "/status", `unknown field "extra"`} {
		if !strings.Contains(joined, w) {
			t.Errorf("missing %q in:\n%s", w, joined)
		}
	}
	for _, v := range vs {
		if strings.Contains(v.Message, `"extra"`) {
			if l, c := doc.Src.Position(v.Start); l != 3 || c != 16 {
				t.Errorf("extra key position %d:%d", l, c)
			}
		}
	}
	ok := parse(t, `{"title":"A","kind":"requirement","status":"draft","statement":"abcd","notes":["n"],"hidden":true,
		"parent":"x","links":["y"],"tags":["t"],"body":"b","order":2,"reviewed":{},"docIgnore":[]}`)
	if vs := s.CheckKind("requirement", ok.Root); len(vs) != 0 {
		t.Errorf("service fields must be allowed by a closed kind: %+v", vs)
	}
}

func messages(vs []Violation) string {
	var msgs []string
	for _, v := range vs {
		msgs = append(msgs, v.Message+" | "+v.Hint)
	}
	return strings.Join(msgs, "\n")
}

// The allowed names come from properties, allOf branches and local $ref
// targets (followed recursively), not from if/then.
func TestAllowedNames(t *testing.T) {
	s, err := load(t, map[string]string{
		"req.json": `{"properties":{"a":{}},"allOf":[{"properties":{"b":{}}},{"$ref":"parts/c.json"}],
			"if":{"required":["a"]},"then":{"properties":{"hidden-x":{}}},"additionalProperties":false}`,
		"parts/c.json": `{"properties":{"c":{"x-docc-target":["goal"]}},"$ref":"d.json"}`,
		"parts/d.json": `{"properties":{"d":{}},"allOf":[{"$ref":"#/$defs/e"}],"$defs":{"e":{"properties":{"e":{}}}}}`,
	})
	if err != nil {
		t.Fatal(err)
	}
	k := s.Kind("req")
	for _, n := range []string{"a", "b", "c", "d", "e"} {
		if !k.allowed[n] {
			t.Errorf("%s is not allowed: %v", n, k.allowed)
		}
	}
	if k.allowed["hidden-x"] {
		t.Error("if/then properties must not count")
	}
	if got := k.Targets("c"); len(got) != 1 || got[0] != "goal" {
		t.Errorf("targets of c: %v", got)
	}
	vs := s.CheckKind("req", parse(t, `{"title":"T","a":1,"b":1,"c":1,"d":1,"e":1,"hidden-x":1}`).Root)
	if len(vs) != 1 || !strings.Contains(vs[0].Message, `unknown field "hidden-x"`) {
		t.Errorf("%s", messages(vs))
	} // a $ref cycle does not hang the walk
	_, _ = load(t, map[string]string{"loop.json": `{"allOf":[{"$ref":"#"}],"$ref":"loop.json","additionalProperties":true}`})
}

// A typo in a closed kind is reported once with the closest name.
func TestUnknownFieldSuggestion(t *testing.T) {
	s, err := load(t, map[string]string{
		"req.json":  `{"properties":{"statement":{"type":"string"}},"additionalProperties":false,"x-docc-covered":true}`,
		"open.json": `{"additionalProperties":true}`,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !s.Kind("req").Covered || s.Kind("open").Covered {
		t.Error("x-docc-covered")
	}
	vs := s.CheckKind("req", parse(t, `{"title":"T","statment":"x","zzzzzzzz":1,"parnt":"p"}`).Root)
	joined := messages(vs)
	if len(vs) != 3 || !strings.Contains(joined, `Did you mean "statement"?`) || !strings.Contains(joined, `Did you mean "parent"?`) ||
		strings.Count(joined, "Did you mean") != 2 {
		t.Errorf("%s", joined)
	}
	if vs := s.CheckKind("open", parse(t, `{"title":"T","statment":"x"}`).Root); len(vs) != 0 {
		t.Errorf("open kind: %s", messages(vs))
	}
}

// A failing allOf branch must not turn its properties into unknown fields:
// the document missing "parent" gets exactly one error.
func TestClosedKindNoCascade(t *testing.T) {
	s, err := load(t, map[string]string{
		"req.json": `{"type":"object","allOf":[{"required":["parent"],"properties":{"statement":{"type":"string"},"status":{"enum":["draft"]}}}],
			"additionalProperties":false}`,
	})
	if err != nil {
		t.Fatal(err)
	}
	vs := s.CheckKind("req", parse(t, `{"title":"T","statement":"s","status":"draft","notes":["n"]}`).Root)
	if len(vs) != 1 || !strings.Contains(vs[0].Message, `missing property "parent"`) {
		t.Errorf("%s", messages(vs))
	}
	vs = s.CheckKind("req", parse(t, `{"title":"T","parent":"p","statement":"s","status":"wip"}`).Root)
	if len(vs) != 1 || !strings.Contains(vs[0].Message, "/status") {
		t.Errorf("status restricted: %s", messages(vs))
	}
}

func TestAdditionalPropertiesRequired(t *testing.T) {
	for name, files := range map[string]map[string]string{
		"missing":     {"a.json": `{"type":"object"}`},
		"not boolean": {"a.json": `{"additionalProperties":{"type":"string"}}`},
		"bad target":  {"a.json": `{"properties":{"p":{"x-docc-target":"goal"}},"additionalProperties":true}`},
		"bad covered": {"a.json": `{"x-docc-covered":"yes","additionalProperties":true}`},
	} {
		_, err := load(t, files)
		if err == nil {
			t.Errorf("%s: expected error", name)
			continue
		}
		if name != "bad target" && name != "bad covered" && !strings.Contains(err.Error(), "additionalProperties") {
			t.Errorf("%s: %v", name, err)
		}
	}
	if _, err := load(t, map[string]string{"a.json": `{"additionalProperties":true}`, "sub/b.json": `{"type":"string"}`}); err != nil {
		t.Errorf("a file in a subfolder is not a kind: %v", err)
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
  "roots": ["index"], "tags": ["draft"], "recommended": true,
  "rules": {
    "text/forbidden-words": { "level": "warn", "words": ["a"], "allow": [] },
    "text/repeated-word": { "level": "warn", "allow": ["that"] },
    "markdown/raw-html": "off", "style": "warn", "custom/x": "off", "DOC800": "off"
  },
  "customRules": [{ "id": "custom/no-vague", "level": "warn", "kind": "requirement", "given": "/statement",
    "forbid": "x", "message": "m", "hint": "h" }],
  "trace": { "scan": ["tests/**"] }, "maxWarnings": 3
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
		"retired rule":  `{"rules":{"DOC302":"off"}}`,
		"nursery":       `{"rules":{"nursery":"warn"}}`,
		"overrides":     `{"overrides":[]}`,
		"kinds":         `{"kinds":["note"]}`,
		"schema key":    `{"$schema":"x"}`,
		"max param":     `{"rules":{"DOC301":{"max":1}}}`,
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
	r := parse2(t, []byte(`{"title":"A","docIgnore":[{"rule":"DOC301","reason":"x"}]}`))
	if err := sc.Validate(loader.Value(r.Root)); err != nil {
		t.Fatal(err)
	}
}

func TestFormatIsAnAssertion(t *testing.T) {
	s, err := load(t, map[string]string{"event.json": `{"properties":{"due":{"type":"string","format":"date"}},"additionalProperties":true}`})
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
