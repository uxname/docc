package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

var update = flag.Bool("update", false, "rewrite expected.json files")

type goldenDiag struct {
	Code  string `json:"code"`
	Rule  string `json:"rule"`
	Level string `json:"level"`
}

// TestGolden runs every testdata/<CODE>/{red,green} case: the project in the
// directory is checked with `docc <args> --format json` (args from args.txt,
// default "validate") and stdout is compared to expected.json.
// A red case must report its code, a green case must not.
func TestGolden(t *testing.T) {
	root, _ := filepath.Abs("../../testdata")
	codes, err := os.ReadDir(root)
	if err != nil {
		t.Skip("no testdata")
	}
	for _, c := range codes {
		if !c.IsDir() {
			continue
		}
		for _, kind := range []string{"red", "green"} {
			dir := filepath.Join(root, c.Name(), kind)
			if _, err := os.Stat(dir); err != nil {
				continue
			}
			t.Run(c.Name()+"/"+kind, func(t *testing.T) { runGolden(t, dir, c.Name(), kind) })
		}
	}
}

func runGolden(t *testing.T, dir, code, kind string) {
	args := []string{"validate"}
	if b, err := os.ReadFile(filepath.Join(dir, "args.txt")); err == nil {
		args = strings.Fields(string(b))
	}
	args = append(args, "--format", "json")
	t.Chdir(dir)
	var out, errb bytes.Buffer
	exit := run(args, &out, &errb)
	if exit == 2 {
		t.Fatalf("usage/config error: %s", errb.String())
	}
	exp := filepath.Join(dir, "expected.json")
	if *update {
		if err := os.WriteFile(exp, out.Bytes(), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	want, err := os.ReadFile(exp)
	if err != nil {
		t.Fatalf("missing expected.json (run with -update): %v", err)
	}
	if !bytes.Equal(want, out.Bytes()) {
		t.Fatalf("output differs from expected.json\n--- got\n%s\n--- want\n%s", out.String(), want)
	}
	var ds []goldenDiag
	if err := json.Unmarshal(out.Bytes(), &ds); err != nil {
		t.Fatal(err)
	}
	found, hasErr := false, false
	for _, d := range ds {
		if d.Code == code || (code == "DOC800" && strings.HasPrefix(d.Rule, "custom/")) {
			found = true
		}
		if d.Level == "error" {
			hasErr = true
		}
	}
	switch kind {
	case "red":
		if !found {
			t.Errorf("red case does not report %s", code)
		}
	case "green":
		if found {
			t.Errorf("green case reports %s", code)
		}
	}
	wantExit := 0
	if hasErr {
		wantExit = 1
	}
	if exit != wantExit {
		t.Errorf("exit code %d, want %d", exit, wantExit)
	}
}

// TestRenderGolden builds testdata/render/<case> (normal and --full) and
// compares the generated trees with expect/out and expect/out-full.
func TestRenderGolden(t *testing.T) {
	root, _ := filepath.Abs("../../testdata/render")
	cases, err := os.ReadDir(root)
	if err != nil {
		t.Skip("no render testdata")
	}
	for _, c := range cases {
		if !c.IsDir() {
			continue
		}
		t.Run(c.Name(), func(t *testing.T) {
			src := filepath.Join(root, c.Name())
			work := t.TempDir()
			if err := copyTree(filepath.Join(src, "docs"), filepath.Join(work, "docs")); err != nil {
				t.Fatal(err)
			}
			if _, err := os.Stat(filepath.Join(src, "docc.json")); err == nil {
				b, _ := os.ReadFile(filepath.Join(src, "docc.json"))
				_ = os.WriteFile(filepath.Join(work, "docc.json"), b, 0o644)
			}
			t.Chdir(work)
			for _, mode := range []struct {
				args        []string
				out, expect string
			}{
				{[]string{"build"}, "docs/out", "out"},
				{[]string{"build", "--full"}, "docs/out-full", "out-full"},
			} {
				var out, errb bytes.Buffer
				if code := run(mode.args, &out, &errb); code == 2 {
					t.Fatalf("%v: %s", mode.args, errb.String())
				}
				expectDir := filepath.Join(src, "expect", mode.expect)
				if *update {
					_ = os.RemoveAll(expectDir)
					if err := copyTree(filepath.Join(work, mode.out), expectDir); err != nil {
						t.Fatal(err)
					}
				}
				compareTrees(t, expectDir, filepath.Join(work, mode.out))
			}
		})
	}
}

func copyTree(from, to string) error {
	return filepath.WalkDir(from, func(p string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(from, p)
		dst := filepath.Join(to, rel)
		if d.IsDir() {
			return os.MkdirAll(dst, 0o755)
		}
		b, err := os.ReadFile(p)
		if err != nil {
			return err
		}
		return os.WriteFile(dst, b, 0o644)
	})
}

func readTree(t *testing.T, dir string) map[string]string {
	t.Helper()
	m := map[string]string{}
	_ = filepath.WalkDir(dir, func(p string, d os.DirEntry, err error) error {
		if err == nil && !d.IsDir() {
			rel, _ := filepath.Rel(dir, p)
			b, _ := os.ReadFile(p)
			m[filepath.ToSlash(rel)] = string(b)
		}
		return nil
	})
	return m
}

func compareTrees(t *testing.T, wantDir, gotDir string) {
	t.Helper()
	want, got := readTree(t, wantDir), readTree(t, gotDir)
	for name, w := range want {
		g, ok := got[name]
		if !ok {
			t.Errorf("%s: not generated", name)
		} else if g != w {
			t.Errorf("%s differs\n--- got\n%s\n--- want\n%s", name, g, w)
		}
	}
	for name := range got {
		if _, ok := want[name]; !ok {
			t.Errorf("%s: unexpected file", name)
		}
	}
}
