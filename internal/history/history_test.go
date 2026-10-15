package history

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/uxname/docc/internal/loader"
)

// A paragraph inserted above an edited one does not move the history of the
// edited one onto its neighbour; a removed paragraph is reported with its text;
// an uncommitted edit is the last version.
func TestHistoryFollowsBlocks(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("no git")
	}
	dir := t.TempDir()
	src := filepath.Join(dir, "docs", "src")
	if err := os.MkdirAll(src, 0o755); err != nil {
		t.Fatal(err)
	}
	run := func(args ...string) {
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		cmd.Env = append(os.Environ(), "GIT_AUTHOR_NAME=A", "GIT_AUTHOR_EMAIL=a@x", "GIT_COMMITTER_NAME=A", "GIT_COMMITTER_EMAIL=a@x",
			"GIT_AUTHOR_DATE=2026-10-01T10:00:00Z", "GIT_COMMITTER_DATE=2026-10-01T10:00:00Z")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %s", args, out)
		}
	}
	write := func(s string) {
		if err := os.WriteFile(filepath.Join(src, "a.json"), []byte(s), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	commit := func(msg string) { run("add", "-A"); run("commit", "-q", "-m", msg) }
	run("init", "-q")
	write(`{"title":"A","body":["one","two","three"],"f":"x"}`)
	commit("first")
	write(`{"title":"A","body":["one","new","two edited","three"],"f":"x"}`)
	commit("second")
	write(`{"title":"A","body":["one","new","two edited"],"f":"y"}`)
	commit("third")
	write(`{"title":"A","body":["one!","new","two edited"],"f":"y"}`)
	cur := loader.Parse([]byte(`{"title":"A","body":["one!","new","two edited"],"f":"y"}`)).Root

	h, err := Load(src, map[string]*loader.Node{"a.json": cur}, Options{Full: true})
	if err != nil || h == nil || len(h.Commits) != 4 || h.Commits[3].Hash != "" || h.Commits[1].Subject != "second" {
		t.Fatalf("commits: %+v", h)
	}
	d := h.Docs["a.json"]
	if vs := d.Blocks["/body/2"]; len(vs) != 2 || vs[0].Text != "two" || vs[1].Text != "two edited" || vs[1].C != 1 {
		t.Errorf("edited paragraph: %+v", vs)
	}
	if vs := d.Blocks["/body/1"]; len(vs) != 1 || vs[0].C != 1 {
		t.Errorf("inserted paragraph: %+v", vs)
	}
	if vs := d.Blocks["/body/0"]; len(vs) != 2 || vs[1].C != 3 {
		t.Errorf("uncommitted edit: %+v", vs)
	}
	if vs := d.Blocks["/f"]; len(vs) != 2 || vs[1].Text != "y" {
		t.Errorf("field: %+v", vs)
	}
	if len(d.Changes) != 4 || !d.Changes[0].Created || len(d.Changes[2].Removed) != 1 || d.Changes[2].Removed[0][1] != "three" {
		t.Errorf("changes: %+v", d.Changes)
	}
}

func TestNoRepository(t *testing.T) {
	if h, err := Load(t.TempDir(), nil, Options{}); h != nil || err != nil {
		t.Errorf("history outside a repository: %+v %v", h, err)
	}
	if _, err := Load(t.TempDir(), nil, Options{Since: "v1"}); err == nil {
		t.Error("changes since a revision outside a repository must be an error")
	}
}

// The file for the reader has no hidden fields, no authors or messages of
// the commits and nothing before the starting point; the full build keeps all.
func TestHistoryForTheReader(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("no git")
	}
	dir := t.TempDir()
	src := filepath.Join(dir, "docs", "src")
	if err := os.MkdirAll(src, 0o755); err != nil {
		t.Fatal(err)
	}
	run := func(args ...string) {
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		cmd.Env = append(os.Environ(), "GIT_AUTHOR_NAME=Secret Author", "GIT_AUTHOR_EMAIL=a@x", "GIT_COMMITTER_NAME=A", "GIT_COMMITTER_EMAIL=a@x")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %s", args, out)
		}
	}
	step := func(doc, msg string) {
		if err := os.WriteFile(filepath.Join(src, "a.json"), []byte(doc), 0o644); err != nil {
			t.Fatal(err)
		}
		run("add", "-A")
		run("commit", "-q", "-m", msg)
	}
	run("init", "-q")
	step(`{"title":"A","body":["draft one"],"notes":["internal guess"]}`, "draft by the agents")
	step(`{"title":"A","body":["sent one"],"notes":["internal guess 2"]}`, "sent to the customer")
	run("tag", "sent")
	step(`{"title":"A","body":["sent one","added later"],"notes":["internal guess 3"]}`, "after the call")
	cur := loader.Parse([]byte(`{"title":"A","body":["sent one","added later"],"notes":["internal guess 3"]}`)).Root
	files := map[string]*loader.Node{"a.json": cur}

	h, err := Load(src, files, Options{Since: "sent"})
	if err != nil {
		t.Fatal(err)
	}
	js, _ := json.Marshal(h)
	for _, leak := range []string{"internal guess", "/notes", "Secret Author", "draft by the agents", "after the call", "draft one"} {
		if strings.Contains(string(js), leak) {
			t.Errorf("the reader's history holds %q:\n%s", leak, js)
		}
	}
	d := h.Docs["a.json"]
	if !h.Base || len(h.Commits) != 2 {
		t.Fatalf("commits from the starting point: %+v", h.Commits)
	}
	if vs := d.Blocks["/body/0"]; len(vs) != 1 || vs[0].C != 0 || vs[0].Text != "sent one" {
		t.Errorf("block as sent: %+v", vs)
	}
	if vs := d.Blocks["/body/1"]; len(vs) != 1 || vs[0].C != 1 {
		t.Errorf("block added after: %+v", vs)
	}
	if len(d.Changes) != 1 || d.Changes[0].C != 1 || len(d.Changes[0].Added) != 1 {
		t.Errorf("changes after the starting point: %+v", d.Changes)
	}
	if _, err := Load(src, files, Options{Since: "no-such-tag"}); err == nil || !strings.Contains(err.Error(), "no such Git revision") {
		t.Errorf("unknown revision: %v", err)
	}
	full, _ := Load(src, files, Options{Full: true})
	if js, _ := json.Marshal(full); !strings.Contains(string(js), "internal guess 3") || !strings.Contains(string(js), "Secret Author") {
		t.Errorf("the full build keeps the notes and authors:\n%s", js)
	}
}

// A merge is one change on the main line: a clean tree after a merge has no
// uncommitted commit, and changesSince must be an ancestor of HEAD.
func TestHistoryAcrossMerge(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("no git")
	}
	dir := t.TempDir()
	src := filepath.Join(dir, "docs", "src")
	if err := os.MkdirAll(src, 0o755); err != nil {
		t.Fatal(err)
	}
	run := func(args ...string) {
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		cmd.Env = append(os.Environ(), "GIT_AUTHOR_NAME=A", "GIT_AUTHOR_EMAIL=a@x", "GIT_COMMITTER_NAME=A", "GIT_COMMITTER_EMAIL=a@x")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %s", args, out)
		}
	}
	step := func(doc, msg string) {
		if err := os.WriteFile(filepath.Join(src, "a.json"), []byte(doc), 0o644); err != nil {
			t.Fatal(err)
		}
		run("add", "-A")
		run("commit", "-q", "-m", msg)
	}
	// the title and the last paragraph are far apart, so Git merges them
	doc := func(title, extra, msg string) (string, string) {
		last := ""
		if extra != "" {
			last = `,"` + extra + `"`
		}
		return `{"title":"` + title + `",` + "\n\n\n\n\n" + `"body":["A",` + "\n\n\n\n\n" + `"B"` + last + "]}\n", msg
	}
	run("init", "-q", "-b", "main")
	step(doc("A", "", "base"))
	run("checkout", "-q", "-b", "feat")
	step(doc("A", "F-para", "feature"))
	run("checkout", "-q", "main")
	step(doc("A changed", "", "main edit"))
	run("tag", "onmain")
	run("merge", "-q", "--no-edit", "feat")
	data, _ := os.ReadFile(filepath.Join(src, "a.json"))
	merged := string(data)
	if !strings.Contains(merged, "A changed") || !strings.Contains(merged, "F-para") {
		t.Fatalf("merge result: %s", data)
	}
	files := map[string]*loader.Node{"a.json": loader.Parse(data).Root}
	h, err := Load(src, files, Options{})
	if err != nil || h.Commits[len(h.Commits)-1].Hash == "" {
		t.Fatalf("a clean tree after a merge has no uncommitted commit: %+v %v", h.Commits, err)
	}
	h, err = Load(src, files, Options{Since: "onmain"})
	if err != nil {
		t.Fatal(err)
	}
	d := h.Docs["a.json"]
	if vs := d.Blocks["/body/2"]; len(vs) != 1 || vs[0].C == 0 {
		t.Errorf("the merged paragraph is new since the tag: %+v", vs)
	}
	if _, err := Load(src, files, Options{Since: "feat"}); err != nil {
		t.Errorf("merged branch is an ancestor of HEAD: %v", err)
	}
	run("checkout", "-q", "-b", "other", "main~2")
	step(`{"title":"Other","body":["A"]}`, "other")
	run("checkout", "-q", "main")
	if _, err := Load(src, files, Options{Since: "other"}); err == nil || !strings.Contains(err.Error(), "not in the history") {
		t.Errorf("a revision outside the history of HEAD: %v", err)
	}
}
