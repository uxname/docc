package main

import (
	"encoding/json"
	"os"
	"regexp"
	"strings"
	"testing"
)

// app is the HTML file split for tests: the shell (template, styles and
// scripts), the data island, and the rendered HTML of every block and property.
type app struct {
	Shell, Blocks string
	Data          struct {
		Title string
		Pages []struct {
			ID, Title, Kind, Status, SC, Priority, Parent string
			Tags, Crumbs, Kids, Refs                      []string
			Props                                         []struct{ K, ID, P, H string }
			Blocks                                        []struct{ P, S, H string }
			MD                                            string
		}
		Comments []struct{ ID, Path, Status string }
		Hist     *struct{ Commits []struct{ H string } }
		Single   string
		Brand    struct{ Head, Print, Foot string }
	}
}

var island = regexp.MustCompile(`(?s)<script type="application/json" id="docc-data">(.*?)</script>`)

func readApp(t *testing.T, path string) app {
	t.Helper()
	s := readFile(t, path)
	m := island.FindStringSubmatchIndex(s)
	if m == nil {
		t.Fatalf("%s has no data island", path)
	}
	var a app
	a.Shell = s[:m[2]] + s[m[3]:]
	if err := json.Unmarshal([]byte(s[m[2]:m[3]]), &a.Data); err != nil {
		t.Fatalf("%s: data: %v", path, err)
	}
	var b strings.Builder
	for _, p := range a.Data.Pages {
		for _, pr := range p.Props {
			b.WriteString(pr.H + "\n")
		}
		for _, bl := range p.Blocks {
			b.WriteString(bl.H + "\n")
		}
	}
	a.Blocks = b.String()
	return a
}

// Blocks carry the JSON Pointer and source text of their element; headings
// have none; a block marker written by an agent is dropped like other raw HTML.
func TestHTMLBlocksHavePointers(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","body":["One.","<!--b:0-->","Two."],"field":"Free text.","list":{"a":"x"}}`,
	})
	if code, out, errs := do("build", "--html"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	a := readApp(t, "docs/out.html")
	var got []string
	for _, b := range a.Data.Pages[0].Blocks {
		got = append(got, b.P+"="+b.S)
	}
	want := "/body/0=One. /body/1=<!--b:0--> /body/2=Two. = /field=Free text. = /list=a: x"
	if strings.Join(got, " ") != want {
		t.Errorf("blocks:\n got %s\nwant %s", strings.Join(got, " "), want)
	}
	if !strings.Contains(a.Blocks, "<!-- raw HTML omitted -->") || strings.Contains(a.Blocks, "<!--b:") {
		t.Errorf("a marker in the text must be dropped:\n%s", a.Blocks)
	}
}

// The whole review loop: a comments file from the page is imported, listed with
// its block, found again after the block moved, marked done and deleted.
func TestComments(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"I","body":["Alpha text here.","Beta text here."]}`,
		"export.json": `{"docc":"comments","version":1,"comments":[
			{"id":"aaaa1111","doc":"index","path":"/body/1","quote":"Beta text here.","text":"Fix beta","author":"C","created":"2026-10-15T10:00:00Z"},
			{"id":"bbbb2222","doc":"index","path":"/body/0","quote":"Alpha text here.","text":"Fix alpha"}]}`,
		"bad.json": `{"comments":[]}`,
	})
	if code, out, errs := do("comments", "import", "export.json"); code != 0 || !strings.Contains(out, "imported 2 comments") {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	if code, out, _ := do("comments", "import", "export.json"); code != 0 || !strings.Contains(out, "imported 0 comments into docc.comments.json (2 already there)") {
		t.Fatalf("second import: %d %s", code, out)
	}
	if code, _, errs := do("comments", "import", "bad.json"); code != 2 || !strings.Contains(errs, "not a comments file") {
		t.Fatalf("bad file: %d %s", code, errs)
	}
	// an agent inserts a paragraph above beta and edits alpha
	if err := os.WriteFile("docs/src/index.json", []byte(`{"title":"I","kind":"page","body":["Alpha text here, edited.","Inserted.","Beta text here."]}`), 0o644); err != nil {
		t.Fatal(err)
	}
	code, out, _ := do("comments")
	for _, want := range []string{
		"aaaa1111  open  index \"I\" /body/2\n  C 2026-10-15: Fix beta\n  block: Beta text here.",
		"bbbb2222  open  index \"I\" /body/0\n  Fix alpha\n  block when commented: Alpha text here.\n  block now: Alpha text here, edited.",
		"2 not processed, 2 in all (docc.comments.json)",
	} {
		if code != 0 || !strings.Contains(out, want) {
			t.Errorf("list lacks %q:\n%s", want, out)
		}
	}
	if code, out, _ := do("comments", "done", "aaaa"); code != 0 || out != "processed aaaa1111\n" {
		t.Fatalf("done: %d %q", code, out)
	}
	if code, _, errs := do("comments", "done", "zz"); code != 2 || !strings.Contains(errs, `no comment "zz"`) {
		t.Fatalf("unknown id: %d %s", code, errs)
	}
	_, out, _ = do("comments", "--format", "json")
	var list struct {
		Open     int
		Comments []struct{ ID, State string }
	}
	if err := json.Unmarshal([]byte(out), &list); err != nil || list.Open != 1 || len(list.Comments) != 1 || list.Comments[0].State != "changed" {
		t.Fatalf("%v %s", err, out)
	}
	// the HTML file shows both on their blocks, beta at its new place
	if code, out, errs := do("build", "--html"); code != 0 {
		t.Fatal(out, errs)
	}
	a := readApp(t, "docs/out.html")
	if len(a.Data.Comments) != 2 || a.Data.Comments[0].Path != "/body/2" || a.Data.Comments[0].Status != "done" {
		t.Errorf("comments in the page: %+v", a.Data.Comments)
	}
	if code, out, _ := do("comments", "rm", "bbbb2222", "aaaa1111"); code != 0 || out != "deleted bbbb2222\ndeleted aaaa1111\n" {
		t.Fatalf("rm: %d %q", code, out)
	}
	if got := readFile(t, "docc.comments.json"); got != "{\"comments\": []}\n" {
		t.Errorf("after rm: %q", got)
	}
}
