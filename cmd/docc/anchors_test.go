package main

import (
	"strings"
	"testing"
)

func TestBlockAnchorsCheck(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index ^t","body":["Release ^new","x ^2","{{index#^new}}"],
			"rows":[{"k":"cell ^cell"}],"obj":{"k":"member ^member"},"single":"See [[a]]. ^one"}`,
		"docs/src/a.json": `{"title":"A","parent":"index","body":"[[index#^new]] [[index#^one]] [[index#^cell]] [[index#^member]] [[index#^t]]"}`,
	})
	_, out, _ := do("validate")
	if strings.Contains(out, "DOC353") {
		t.Errorf("the marker ^new reaches text rules:\n%s", out)
	}
	if strings.Contains(out, "DOC211") {
		t.Errorf("an include of a paragraph of the same field is taken for a cycle:\n%s", out)
	}
	if n := strings.Count(out, "DOC312"); n != 3 || strings.Contains(out, `"^new"`) || strings.Contains(out, `"^one"`) {
		t.Errorf("want DOC312 for ^cell, ^member and ^t only:\n%s", out)
	}
}

func TestBlockAnchorIncludeCycle(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index","body":["See [[a]].","Loop {{a#^b}} ^a"]}`,
		"docs/src/a.json":     `{"title":"A","parent":"index","body":"Back {{index#^a}} ^b"}`,
	})
	if code, out, _ := do("validate"); code == 0 || !strings.Contains(out, "include cycle") {
		t.Errorf("a cycle through block anchors is missed: %d %s", code, out)
	}
}
