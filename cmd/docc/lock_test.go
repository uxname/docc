package main

import (
	"os"
	"strings"
	"testing"
)

func readLock(t *testing.T) string {
	t.Helper()
	b, err := os.ReadFile("docc.lock")
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func write(t *testing.T, name, content string) {
	t.Helper()
	if err := os.WriteFile(name, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}

func mustBuild(t *testing.T) {
	t.Helper()
	if code, out, errs := do("build"); code != 0 {
		t.Fatalf("build: %d %s %s", code, out, errs)
	}
}

func TestLockSuspectAndAck(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index","body":"See [[a]] and [[a#^limit]] and [[a#spec/Rate limits]]. [[index]]","notes":["[[b]]"]}`,
		"docs/src/a.json":     `{"title":"A","parent":"index","body":["Five per minute. ^limit","Other."],"spec":{"$sections":true,"Rate limits":"R"}}`,
		"docs/src/b.json":     `{"title":"B","parent":"index","hidden":true,"body":"[[a]]"}`,
	})
	mustBuild(t)
	lock := readLock(t)
	for _, l := range []string{"b a ", "index a ", "index a#^limit ", "index a#spec/Rate%20limits ", "index b "} {
		if !strings.Contains(lock, "\n"+l) && !strings.HasPrefix(lock, l) {
			t.Errorf("no line %q:\n%s", l, lock)
		}
	}
	if strings.Contains(lock, "index index") || strings.Count(lock, "\n") != 5 {
		t.Errorf("self link or wrong count:\n%s", lock)
	}
	write(t, "docs/src/a.json", `{"kind":"page","title":"A","parent":"index","order":2,"body":["Ten per minute. ^limit","Other."],"spec":{"$sections":true,"Rate limits":"R"}}`)
	_, out, _ := do("validate")
	if strings.Count(out, "DOC209") != 3 || !strings.Contains(out, "docs/src/index.json") || !strings.Contains(out, "docs/src/b.json") {
		t.Fatalf("want DOC209 for a and a#^limit from index and a from b:\n%s", out)
	}
	if _, out, _ := do("check"); strings.Contains(out, "DOC503") || strings.Contains(out, "DOC109") {
		t.Errorf("the lock is fresh:\n%s", out)
	}
	if code, out, _ := do("ack", "a#^limit"); code != 0 || !strings.Contains(out, "confirmed 1 links") {
		t.Fatalf("ack: %d %s", code, out)
	}
	if _, out, _ = do("validate"); strings.Count(out, "DOC209") != 2 {
		t.Fatalf("ack of a#^limit must clear only that line:\n%s", out)
	}
	if code, out, _ := do("ack", "a"); code != 0 || !strings.Contains(out, "confirmed 4 links") {
		t.Fatalf("ack a: %d %s", code, out)
	}
	if _, out, _ = do("validate"); strings.Contains(out, "DOC209") {
		t.Fatalf("ack a must confirm every link to a:\n%s", out)
	}
	if code, _, errs := do("ack", "nothing"); code != 2 || !strings.Contains(errs, "nothing") {
		t.Errorf("ack of an unknown name: %d %s", code, errs)
	}
	if code, _, _ := do("ack"); code != 2 {
		t.Error("ack without arguments must fail")
	}
}

func TestLockTitleTypoOnlyWholeDoc(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index","body":"[[a]] [[a#^limit]]"}`,
		"docs/src/a.json":     `{"title":"A","parent":"index","body":"Five. ^limit"}`,
	})
	mustBuild(t)
	write(t, "docs/src/a.json", `{"kind":"page","title":"Aa","parent":"index","body":"Five. ^limit"}`)
	_, out, _ := do("validate")
	if strings.Count(out, "DOC209") != 1 || !strings.Contains(out, "the target a changed") {
		t.Fatalf("want DOC209 for [[a]] only:\n%s", out)
	}
}

func TestLockCode(t *testing.T) {
	project(t, map[string]string{
		"docc.json":           `{"trace":{"scan":["code/**"]}}`,
		"docs/src/index.json": `{"title":"Index","body":"Five. ^limit"}`,
		"code/a.go":           "package a\n// docc:begin index#^limit\nconst a = 5\n// docc:end\n\n// docc:begin index#^limit\nconst b = 6\n// docc:end\n// docc: index\n",
	})
	mustBuild(t)
	if lock := readLock(t); strings.Count(lock, "\n") != 2 || !strings.Contains(lock, "code/a.go index#^limit ") || len(strings.Fields(strings.Split(lock, "\n")[0])) != 3 {
		t.Fatalf("want one line per target, code hash only for the region:\n%s", lock)
	}
	write(t, "code/a.go", "package a\n\nfunc f() {}\n// docc:begin index#^limit\n   const a = 5\n// docc:end\n\n// docc:begin index#^limit\nconst b = 6\n// docc:end\n// docc: index\n")
	if code, out, _ := do("check"); code != 0 || strings.Contains(out, "DOC402") {
		t.Fatalf("code around the region or indentation changed nothing: %d\n%s", code, out)
	}
	write(t, "code/a.go", "package a\n// docc:begin index#^limit\nconst a = 5\n// docc:end\n\n// docc:begin index#^limit\nconst b = 7\n// docc:end\n// docc: index\n")
	_, out, _ := do("validate")
	if !strings.Contains(out, "DOC402") || !strings.Contains(out, "code/a.go:2") {
		t.Fatalf("want DOC402 at the first region tag:\n%s", out)
	}
	if code, _, _ := do("ack", "code/a.go"); code != 0 {
		t.Fatal(code)
	}
	if _, out, _ = do("validate"); strings.Contains(out, "DOC402") {
		t.Fatalf("ack did not confirm the region:\n%s", out)
	}
}

func TestLockMergeProblems(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index","body":"[[a]] [[b]]"}`,
		"docs/src/a.json":     `{"title":"A","parent":"index"}`,
		"docs/src/b.json":     `{"title":"B","parent":"index"}`,
	})
	if code, out, _ := do("check"); code != 1 || !strings.Contains(out, "DOC503") || !strings.Contains(out, "missing") {
		t.Fatalf("no lock with links: %d\n%s", code, out)
	}
	mustBuild(t)
	full := readLock(t)
	lines := strings.SplitAfter(full, "\n")
	stale := strings.Replace(lines[0], lines[0][len(lines[0])-9:len(lines[0])-1], "00000000", 1)
	write(t, "docc.lock", stale)
	if code, out, _ := do("check"); code != 1 || !strings.Contains(out, "index to b is not in docc.lock") {
		t.Fatalf("a lost line: %d\n%s", code, out)
	}
	mustBuild(t)
	if got := readLock(t); got != stale+lines[1] {
		t.Fatalf("build must re-add the lost line and keep the stored hash:\n%s", got)
	}
	write(t, "docc.lock", stale+lines[1]+lines[1])
	if code, out, _ := do("check"); code != 1 || !strings.Contains(out, "listed twice") {
		t.Fatalf("a repeated key: %d\n%s", code, out)
	}
	write(t, "docc.lock", "<<<<<<< HEAD\n"+full)
	if code, out, _ := do("check"); code != 1 || !strings.Contains(out, "line 1 is not") {
		t.Fatalf("a malformed line: %d\n%s", code, out)
	}
	write(t, "docc.lock", lines[0]+lines[0]+lines[1])
	mustBuild(t)
	if readLock(t) != full {
		t.Fatal("build must keep one line per key")
	}
}

func TestLockRenamedSection(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index","body":"[[a#spec/Old]]"}`,
		"docs/src/a.json":     `{"title":"A","parent":"index","spec":{"$sections":true,"Old":"x"}}`,
	})
	mustBuild(t)
	write(t, "docs/src/a.json", `{"kind":"page","title":"A","parent":"index","spec":{"$sections":true,"New":"x"}}`)
	if _, out, _ := do("validate"); !strings.Contains(out, "DOC312") {
		t.Fatalf("a renamed section must break the link:\n%s", out)
	}
	write(t, "docs/src/index.json", `{"kind":"page","title":"Index","body":"[[a#spec/New]]"}`)
	mustBuild(t)
	if lock := readLock(t); lock != strings.Replace(lock, "Old", "", 1) || !strings.HasPrefix(lock, "index a#spec/New ") {
		t.Fatalf("the old line must go:\n%s", lock)
	}
	if code, _, _ := do("check"); code != 0 {
		t.Fatal("check after build")
	}
}
