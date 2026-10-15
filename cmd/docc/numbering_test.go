package main

import (
	"fmt"
	"strings"
	"testing"
)

func numberingProject(t *testing.T, numbering bool) {
	t.Helper()
	cfg := `{"roots":["index","appx"]}`
	if numbering {
		cfg = `{"roots":["index","appx"],"numbering":true}`
	}
	project(t, map[string]string{
		"docc.json":           cfg,
		"docs/src/index.json": `{"title":"Index","body":"See [[b1]], [[b1#spec/Limits/Size]] and [[b1#more/Roles]]."}`,
		"docs/src/a.json":     `{"title":"Zeta","parent":"index","order":1}`,
		"docs/src/h.json":     `{"title":"Hidden","parent":"index","order":2,"hidden":true}`,
		"docs/src/b.json":     `{"title":"Alpha","parent":"index","order":3}`,
		"docs/src/c.json":     `{"title":"Beta","parent":"index"}`,
		"docs/src/b1.json": `{"title":"Deep","parent":"b",
			"spec":{"$sections":true,"Limits":{"$sections":true,"Size":"Up to 5."},"Users":"Twelve."},
			"more":{"$sections":true,"Roles":"Two."}}`,
		"docs/src/appx.json": `{"title":"Appendix"}`,
		"docs/src/x.json":    `{"title":"Extra","parent":"appx"}`,
	})
}

func TestNumbering(t *testing.T) {
	numberingProject(t, true)
	if code, out, errs := do("build"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	tree := readFile(t, "docs/out/_tree.md")
	for _, want := range []string{
		"- [Index](index.md)\n  - [1 Zeta](a.md)\n  - [2 Alpha](b.md)\n    - [2.1 Deep](b1.md)\n  - [3 Beta](c.md)\n- [Appendix](appx.md)\n  - [4 Extra](x.md)",
	} {
		if !strings.Contains(tree, want) {
			t.Errorf("tree lacks %q:\n%s", want, tree)
		}
	}
	page := readFile(t, "docs/out/b1.md")
	for _, want := range []string{"# 2.1 Deep", "## Spec", "<a id=\"limits\"></a>\n\n### 2.1.1 Limits", "<a id=\"size\"></a>\n\n#### 2.1.1.1 Size", "### 2.1.2 Users", "## More", "### 2.1.3 Roles", "[2 Alpha](b.md)"} {
		if !strings.Contains(page, want) {
			t.Errorf("b1.md lacks %q:\n%s", want, page)
		}
	}
	index := readFile(t, "docs/out/index.md")
	if !strings.Contains(index, "# Index\n") || !strings.Contains(index, "See [2.1 Deep](b1.md), [2.1.1.1 Size](b1.md#size) and [2.1.3 Roles](b1.md#roles).") {
		t.Errorf("links lack numbers or the root got one:\n%s", index)
	}
	if code, _, errs := do("build", "--full"); code != 0 {
		t.Fatal(errs)
	}
	full := readFile(t, "docs/out-full/_tree.md")
	if !strings.Contains(full, "  - [1 Zeta](a.md)\n  - [Hidden](h.md) (hidden)\n  - [2 Alpha](b.md)") || !strings.Contains(full, "[4 Extra](x.md)") {
		t.Errorf("--full gives other numbers:\n%s", full)
	}
	if code, _, errs := do("build", "--html"); code != 0 {
		t.Fatal(errs)
	}
	a := readApp(t, "docs/out.html")
	for _, want := range []string{`<h4 id="b1--size">2.1.1.1 Size</h4>`, `<h3 id="b1--roles">2.1.3 Roles</h3>`, `href="#b1--size">2.1.1.1 Size</a>`} {
		if !strings.Contains(a.Blocks, want) {
			t.Errorf("out.html lacks %q", want)
		}
	}
	if !strings.Contains(fmt.Sprint(a.Data.Pages), "b1 2.1 Deep") {
		t.Errorf("the page title lacks its number: %+v", a.Data.Pages)
	}
	if code, out, _ := do("check"); code != 0 {
		t.Errorf("check after build: %d %s", code, out)
	}
}

func TestNumberingOffKeepsTitlesAndAnchors(t *testing.T) {
	numberingProject(t, false)
	if code, _, errs := do("build", "--html"); code != 0 {
		t.Fatal(errs)
	}
	a := readApp(t, "docs/out.html")
	for _, want := range []string{`<h4 id="b1--size">Size</h4>`, `href="#b1--size">Size</a>`} {
		if !strings.Contains(a.Blocks, want) {
			t.Errorf("out.html lacks %q", want)
		}
	}
	if !strings.Contains(fmt.Sprint(a.Data.Pages), "b1 Deep") {
		t.Errorf("page title: %+v", a.Data.Pages)
	}
	do("build")
	if index := readFile(t, "docs/out/index.md"); !strings.Contains(index, "See [Deep](b1.md), [Size](b1.md#size) and [Roles](b1.md#roles).") {
		t.Errorf("numbering off changed links:\n%s", index)
	}
	if tree := readFile(t, "docs/out/_tree.md"); !strings.Contains(tree, "[Zeta](a.md)\n  - [Alpha](b.md)") {
		t.Errorf("order must hold without numbering:\n%s", tree)
	}
	if page := readFile(t, "docs/out/b1.md"); strings.Contains(page, "<a id=") {
		t.Errorf("unnumbered headings need no explicit anchors:\n%s", page)
	}
}
