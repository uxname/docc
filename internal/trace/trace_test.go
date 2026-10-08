package trace

import (
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"testing"

	"github.com/uxname/docc/internal/config"
)

func TestCommentPrefix(t *testing.T) {
	for line, want := range map[string]bool{
		"// docc: a":           true,
		"  # docc: a":          true,
		"-- docc: a":           true,
		"/* docc: a */":        true,
		" * docc: a":           true,
		"; docc: a":            true,
		"<!-- docc: a -->":     true,
		"% docc: a":            true,
		"' docc: a":            true,
		"/** docc: a":          true,
		"docc: a":              true,
		`s := "docc: a"`:       false,
		"x = 1 // docc: a":     false,
		"// see mydocc: a":     false,
		"// text then docc: a": false,
	} {
		tags := scanFile("f", []byte(line))
		if got := len(tags) == 1 && slices.Equal(tags[0].Targets, []string{"a"}); got != want {
			t.Errorf("%q: got %v %+v", line, got, tags)
		}
	}
}

func TestRegions(t *testing.T) {
	tags := scanFile("f", []byte("// docc:begin a, b#^x\n  x := 1  \r\n// docc: c\n// docc:end\n// docc:begin a\ny\n// docc:end\n"))
	if len(tags) != 3 || !tags[0].Region || tags[0].Body != "x := 1\n// docc: c" || tags[2].Body != "y" {
		t.Fatalf("%+v", tags)
	}
	h := Hashes(tags)
	if len(h) != 2 || h[Key{"f", "a"}] == h[Key{"f", "b#^x"}] || h[Key{"f", "c"}] != "" {
		t.Fatalf("a has two regions, b one, c is a plain tag: %v", h)
	}
	// indentation, line ends and Unicode normal form do not change the hash
	same := scanFile("f", []byte("\t// docc:begin b#^x\r\nx := 1\n\t// docc: c\n// docc:end"))
	if Hashes(same)[Key{"f", "b#^x"}] != h[Key{"f", "b#^x"}] {
		t.Error("trim changed the hash")
	}
	nfc := Hashes(scanFile("f", []byte("// docc:begin a\n\u00e9\n// docc:end\n")))
	nfd := Hashes(scanFile("f", []byte("// docc:begin a\ne\u0301\n// docc:end\n")))
	if nfc[Key{"f", "a"}] != nfd[Key{"f", "a"}] {
		t.Error("NFC and NFD differ")
	}
	for src, want := range map[string]string{
		"// docc:begin a\n": "docc:begin without docc:end",
		"// docc:end\n":     "docc:end without docc:begin",
		"// docc:begin a\n// docc:begin b\n// docc:end": "a region opened inside another region",
	} {
		if !slices.ContainsFunc(scanFile("f", []byte(src)), func(t Tag) bool { return t.Err == want }) {
			t.Errorf("%q: want %q", src, want)
		}
	}
}

func TestListing(t *testing.T) {
	write := func(root string, files ...string) {
		for _, f := range files {
			p := filepath.Join(root, f)
			os.MkdirAll(filepath.Dir(p), 0o755)
			if err := os.WriteFile(p, []byte("// docc: x\n"), 0o644); err != nil {
				t.Fatal(err)
			}
		}
	}
	files := func(root string) []string {
		all, _ := config.CompileGlob("**")
		tags, err := Scan(root, []*config.Glob{all}, []string{filepath.Join(root, "docs")})
		if err != nil {
			t.Fatal(err)
		}
		var out []string
		for _, tg := range tags {
			out = append(out, tg.File)
		}
		return out
	}
	plain := t.TempDir()
	write(plain, "a.go", "node_modules/m.js", "vendor/v.go", "docs/d.go")
	if got := files(plain); !slices.Equal(got, []string{"a.go"}) {
		t.Errorf("walk: %v", got)
	}
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("no git")
	}
	repo := t.TempDir()
	write(repo, "a.go", "ignored.go", "vendor/v.go", "docs/d.go", ".gitignore")
	os.WriteFile(filepath.Join(repo, ".gitignore"), []byte("ignored.go\n"), 0o644)
	if out, err := exec.Command("git", "-C", repo, "init", "-q").CombinedOutput(); err != nil {
		t.Fatalf("%s", out)
	}
	if got := files(repo); !slices.Equal(got, []string{"a.go", "vendor/v.go"}) {
		t.Errorf("git: %v", got)
	}
}
