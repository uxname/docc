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
