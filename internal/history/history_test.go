package history

import (
	"os"
	"os/exec"
	"path/filepath"
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

	h := Load(src, map[string]*loader.Node{"a.json": cur})
	if h == nil || len(h.Commits) != 4 || h.Commits[3].Hash != "" || h.Commits[1].Subject != "second" {
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
	if h := Load(t.TempDir(), nil); h != nil {
		t.Errorf("history outside a repository: %+v", h)
	}
}
