package main

import (
	"math/rand"
	"os"
	"path/filepath"
	"testing"
)

// Damaged sources must never crash any command: exit codes stay 0, 1 or 2.
func TestMutatedSourcesDoNotPanic(t *testing.T) {
	base, _ := filepath.Abs("../../testdata/render/basic/docs/src")
	var files []string
	_ = filepath.WalkDir(base, func(p string, d os.DirEntry, err error) error {
		if err == nil && !d.IsDir() {
			files = append(files, p)
		}
		return nil
	})
	rng := rand.New(rand.NewSource(1))
	rounds := 150
	if testing.Short() {
		rounds = 20
	}
	for i := 0; i < rounds; i++ {
		work := t.TempDir()
		for _, f := range files {
			rel, _ := filepath.Rel(base, f)
			data, _ := os.ReadFile(f)
			if rng.Intn(3) == 0 { // damage about a third of the files
				for k := 0; k <= rng.Intn(3); k++ {
					switch rng.Intn(4) {
					case 0:
						if len(data) > 0 {
							j := rng.Intn(len(data))
							data = append(data[:j:j], data[j+1:]...)
						}
					case 1:
						j := rng.Intn(len(data) + 1)
						data = append(data[:j:j], append([]byte{"{}[]\":,\\\n\x00\xff# `<{"[rng.Intn(13)]}, data[j:]...)...)
					case 2:
						data = data[:rng.Intn(len(data)+1)]
					case 3:
						j := rng.Intn(len(data) + 1)
						data = append(data[:j:j], append([]byte("[[x#y]]{{x#y}}"), data[j:]...)...)
					}
				}
			}
			dst := filepath.Join(work, "docs", "src", rel)
			_ = os.MkdirAll(filepath.Dir(dst), 0o755)
			_ = os.WriteFile(dst, data, 0o644)
		}
		t.Chdir(work)
		for _, args := range [][]string{{"validate"}, {"validate", "--fix", "--unsafe"}, {"build"}, {"build", "--full"}, {"check", "--format", "json"}} {
			if code, _, errs := do(args...); code < 0 || code > 2 {
				t.Fatalf("round %d %v: exit %d %s", i, args, code, errs)
			}
		}
	}
}
