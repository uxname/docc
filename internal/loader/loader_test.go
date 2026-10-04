package loader

import (
	"fmt"
	"strings"
	"testing"
)

func pos(r *Result, off int) string {
	l, c := r.Src.Position(off)
	return fmt.Sprintf("%d:%d", l, c)
}

func TestPositionsPlain(t *testing.T) {
	r := Parse([]byte("{\n  \"title\": \"Привет\",\n  \"x\": 1\n}"))
	if r.Err != nil {
		t.Fatal(r.Err)
	}
	title := r.Root.Get("title")
	if got := pos(r, title.Start); got != "2:12" {
		t.Errorf("title start %s", got)
	}
	if got := pos(r, title.End); got != "2:20" { // after 8 runes: quote + 6 + quote
		t.Errorf("title end %s", got)
	}
	if got := pos(r, r.Root.Members[1].KeyStart); got != "3:3" {
		t.Errorf("key x %s", got)
	}
}

func TestBOMAndCRLF(t *testing.T) {
	r := Parse([]byte("\xEF\xBB\xBF{\r\n  \"a\": \"b\"\r\n}"))
	if r.Err != nil || !r.Src.BOM {
		t.Fatal(r.Err, r.Src.BOM)
	}
	if got := pos(r, r.Root.Get("a").Start); got != "2:8" {
		t.Errorf("got %s", got)
	}
	if got := pos(r, r.Root.End-1); got != "3:1" {
		t.Errorf("closing brace %s", got)
	}
}

func TestEscapedStrings(t *testing.T) {
	// "a\nA😀z"  decoded: a, \n, A, 😀 (4 bytes), z
	bs := "\\" // backslash, so escapes stay escapes in the input
	r := Parse([]byte(`{"s":"a` + bs + `n` + bs + `u0041` + bs + `ud83d` + bs + `ude00z"}`))
	if r.Err != nil {
		t.Fatal(r.Err)
	}
	n := r.Root.Get("s")
	if n.Str != "a\nA😀z" {
		t.Fatalf("decoded %q", n.Str)
	}
	want := map[int]string{
		0: "1:7",  // a
		1: "1:8",  // \n escape start
		2: "1:10", // A
		3: "1:16", // emoji (first byte)
		6: "1:16", // emoji (last byte) still the same escape
		7: "1:28", // z
		8: "1:29", // closing quote
	}
	for i, w := range want {
		if got := pos(r, r.Src.StrOffset(n, i)); got != w {
			t.Errorf("index %d: got %s want %s", i, got, w)
		}
	}
}

func TestLiteralAstralRuneCountsOnce(t *testing.T) {
	r := Parse([]byte(`{"s":"😀x"}`))
	n := r.Root.Get("s")
	if got := pos(r, r.Src.StrOffset(n, 4)); got != "1:8" {
		t.Errorf("got %s", got)
	}
}

func TestDuplicateKeys(t *testing.T) {
	r := Parse([]byte("{\"a\":1,\n \"a\":2}"))
	if r.Err != nil || len(r.Dups) != 1 {
		t.Fatal(r.Err, r.Dups)
	}
	if got := pos(r, r.Dups[0].Start); got != "2:2" {
		t.Errorf("got %s", got)
	}
}

func TestErrors(t *testing.T) {
	deep := strings.Repeat("[", 70) + strings.Repeat("]", 70)
	for name, in := range map[string]string{
		"empty":       "",
		"array root":  "[]",
		"comment":     `{"a":1 /* x */}`,
		"trailing":    `{"a":1,}`,
		"leading 0":   `{"a":01}`,
		"ctrl":        "{\"a\":\"x\ny\"}",
		"bad escape":  `{"a":"\x"}`,
		"bad utf8":    "{\"a\":\"\xff\"}",
		"after value": `{} {}`,
		"deep":        `{"a":` + deep + `}`,
	} {
		if r := Parse([]byte(in)); r.Err == nil {
			t.Errorf("%s: expected error", name)
		}
	}
}

func TestValue(t *testing.T) {
	r := Parse([]byte(`{"a":[1,2.5,true,null,"x"],"b":{"c":"d"}}`))
	m := Value(r.Root).(map[string]any)
	if len(m["a"].([]any)) != 5 || m["b"].(map[string]any)["c"] != "d" {
		t.Fatalf("%v", m)
	}
}

func FuzzParse(f *testing.F) {
	for _, s := range []string{
		`{"a":[1,2,{"b":"c"}],"d":"é😀"}`, "{\n\"a\" : 1 ,\r\n\"a\":2}", `{"x":-0.5e+10}`, "\xEF\xBB\xBF{}", `{"s":"\q"}`,
	} {
		f.Add([]byte(s))
	}
	f.Fuzz(func(t *testing.T, data []byte) {
		r := Parse(data)
		if r.Err != nil {
			if r.Err.Offset < 0 || r.Err.Offset > len(r.Src.Data) {
				t.Fatalf("error offset %d out of range", r.Err.Offset)
			}
			return
		}
		var visit func(n *Node)
		visit = func(n *Node) {
			if n.Start < 0 || n.End > len(r.Src.Data) || n.Start >= n.End {
				t.Fatalf("bad range %d..%d", n.Start, n.End)
			}
			r.Src.Position(n.Start)
			if n.Kind == String {
				for i := 0; i <= len(n.Str); i++ {
					off := r.Src.StrOffset(n, i)
					if off < n.Start || off >= n.End {
						t.Fatalf("StrOffset(%d)=%d outside %d..%d", i, off, n.Start, n.End)
					}
				}
			}
			for _, it := range n.Items {
				visit(it)
			}
			for _, m := range n.Members {
				visit(m.Value)
			}
		}
		visit(r.Root)
	})
}
