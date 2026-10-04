package mdscan

import (
	"reflect"
	"strings"
	"testing"
)

func plainText(sc *Scan) string {
	var b strings.Builder
	for _, sp := range sc.Plain {
		b.WriteString(sc.S[sp.Start:sp.End])
		b.WriteByte('|')
	}
	return b.String()
}

func TestPlain(t *testing.T) {
	cases := map[string]string{
		"abc":                        "abc|",
		"a `x` b":                    "a | b|",
		"a ``x ` y`` b":              "a | b|",
		"a `x b":                     "a `x b|",
		"a\n```\ncode [[x]]\n```\nb": "a\n|b|",
		"~~~\ncode\n~~~":             "",
		"a `b\nc` d":                 "a | d|",
		"\\`x` y":                    "\\`x` y|",
	}
	for in, want := range cases {
		if got := plainText(New(in)); got != want {
			t.Errorf("%q: got %q want %q", in, got, want)
		}
	}
}

func TestUnclosedFence(t *testing.T) {
	if sc := New("a\n```go\ncode"); sc.UnclosedFence != 2 {
		t.Errorf("got %d", sc.UnclosedFence)
	}
	if sc := New("```\nx\n```\n"); sc.UnclosedFence != -1 {
		t.Errorf("closed fence reported: %d", sc.UnclosedFence)
	}
	if sc := New("``` inline ``` text"); sc.UnclosedFence != -1 {
		t.Errorf("inline triple ticks: %d", sc.UnclosedFence)
	}
	if sc := New("````\n```\n````"); sc.UnclosedFence != -1 {
		t.Errorf("longer fence containing shorter: %d", sc.UnclosedFence)
	}
}

func TestRefs(t *testing.T) {
	sc := New("see [[a#b]] and `[[no]]` and [[c]] [[bad [x]]")
	var inner []string
	for _, r := range sc.Refs() {
		inner = append(inner, r.Inner)
	}
	if !reflect.DeepEqual(inner, []string{"a#b", "c"}) {
		t.Errorf("%v", inner)
	}
	r := sc.Refs()[0]
	if sc.S[r.Start:r.End] != "[[a#b]]" {
		t.Errorf("range %q", sc.S[r.Start:r.End])
	}
	inc := New("x {{a#b}} `{{no}}`").Includes()
	if len(inc) != 1 || inc[0].Inner != "a#b" {
		t.Errorf("%v", inc)
	}
}

func TestHeadings(t *testing.T) {
	sc := New("text\n# H1\n####### seven\n#nospace\n```\n# code\n```\n## H2 tail")
	var got []string
	for _, h := range sc.Headings() {
		got = append(got, sc.S[h.Start:h.End])
	}
	if !reflect.DeepEqual(got, []string{"# H1", "## H2 tail"}) {
		t.Errorf("%q", got)
	}
}

func TestHTMLTags(t *testing.T) {
	sc := New("a <div class=\"x\">b</div> <id> <Script> `<b>` <br/> <foo>")
	var names []string
	for _, tg := range sc.HTMLTags() {
		names = append(names, tg.Name)
	}
	if !reflect.DeepEqual(names, []string{"div", "script", "br"}) {
		t.Errorf("%v", names)
	}
}

func TestLinks(t *testing.T) {
	sc := New("see [here](https://a.example/x) and https://b.example/y. Also `https://c.example` [ok text](rel.md)")
	ls := sc.Links()
	if len(ls) != 2 || ls[0].Text != "here" || ls[0].URL != "https://a.example/x" || ls[1].Text != "ok text" {
		t.Fatalf("%+v", ls)
	}
	bare := sc.BareURLs()
	if len(bare) != 1 || sc.S[bare[0].Start:bare[0].End] != "https://b.example/y" {
		t.Fatalf("%+v", bare)
	}
}

func TestIncludesAreNotPlainText(t *testing.T) {
	sc := New("a {{x#y}} b `c` {{p#q}}")
	if got := plainText(sc); got != "a | b | |" {
		t.Errorf("plain: %q", got)
	}
	if len(sc.Includes()) != 2 || sc.Includes()[1].Inner != "p#q" {
		t.Errorf("%+v", sc.Includes())
	}
}
