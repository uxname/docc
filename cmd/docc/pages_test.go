package main

import (
	"os"
	"regexp"
	"strings"
	"testing"
)

func TestPageMetaRelationsAndTable(t *testing.T) {
	project(t, map[string]string{
		"docc.json":           `{"trace":{"scan":["code/**"]}}`,
		"code/a.go":           "package a\n\n// @covers req\n",
		"docs/src/index.json": `{"title":"Index","body":"Start.","links":["req"]}`,
		"docs/src/req.json": `{"title":"Req","kind":"requirement","status":"accepted","priority":2,"tags":["x","y"],
			"parent":"index","body":"Text.",
			"acceptance":[{"given":"A","when":"B","then":"C | D"},{"given":"E","when":"F","then":"G"}]}`,
		"docs/src/bare.json": `{"title":"Bare","kind":"requirement","parent":"index"}`,
	})
	if code, out, errs := do("build"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	req := readFile(t, "docs/out/req.md")
	for _, want := range []string{
		"# Req\n\nrequirement · accepted · priority 2 · x, y\n\nText.",
		"| given | when | then |\n| --- | --- | --- |\n| A | B | C \\| D |",
		"## Parent\n\n[Index](index.md)",
		"## Referenced by\n\n- [Index](index.md)",
		"## Covered by\n\n- `code/a.go:3`",
	} {
		if !strings.Contains(req, want) {
			t.Errorf("req.md lacks %q:\n%s", want, req)
		}
	}
	if strings.Contains(req, "## Kind") || strings.Contains(req, "## Tags") || strings.Contains(req, "## Status") {
		t.Errorf("old metadata sections:\n%s", req)
	}
	idx := readFile(t, "docs/out/index.md")
	if !strings.Contains(idx, "## Referenced by\n\n- [Bare](bare.md)\n- [Req](req.md)") || !strings.HasSuffix(idx, "Document tree: [_tree.md](_tree.md)\n") {
		t.Errorf("index.md:\n%s", idx)
	}
	tree := readFile(t, "docs/out/_tree.md")
	if !strings.Contains(tree, "## Requirements without code coverage\n\n- [Bare](bare.md)") || strings.Contains(tree, "- [Req](req.md)\n\n") && strings.Contains(strings.SplitN(tree, "without code coverage", 2)[1], "[Req]") {
		t.Errorf("_tree.md:\n%s", tree)
	}
}

func TestOutputHasNoCyrillic(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","notes":["n"],"reviewed":{"index":"00000000"},"docIgnore":[{"rule":"DOC303","reason":"r","path":"/body"}],"body":"b"}`,
		"docs/src/h.json":     `{"title":"H","hidden":true,"parent":"index"}`,
	})
	do("build")
	do("build", "--full")
	cyr := regexp.MustCompile(`[А-Яа-яЁё]`)
	for _, dir := range []string{"docs/out", "docs/out-full"} {
		ents, err := os.ReadDir(dir)
		if err != nil || len(ents) == 0 {
			t.Fatalf("%s: %v", dir, err)
		}
		for _, e := range ents {
			if s := readFile(t, dir+"/"+e.Name()); cyr.MatchString(s) {
				t.Errorf("%s/%s has Cyrillic:\n%s", dir, e.Name(), s)
			}
		}
	}
	if !strings.Contains(readFile(t, "docs/out-full/_tree.md"), "> full build: includes hidden documents and notes — do not publish") {
		t.Error("full tree banner missing")
	}
}
