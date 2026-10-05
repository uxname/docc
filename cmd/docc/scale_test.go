package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/render"
)

// genScale writes n documents of 5-20 KB of Russian text into dir/docs/src:
// one index, sections below it, leaves below the sections, cross links and a
// kind schema.
func genScale(t testing.TB, dir string, n int) {
	para := "Система должна обеспечивать устойчивую работу при изменении нагрузки на сервис. Администратор получает уведомление о каждом сбое и может открыть журнал событий за выбранный период. "
	write := func(rel, content string) {
		p := filepath.Join(dir, filepath.FromSlash(rel))
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("docs/schemas/note.json", `{"type":"object","properties":{"status":{"enum":["draft","accepted"]},"statement":{"type":"string","minLength":5}},"additionalProperties":true}`)
	write("docs/schemas/page.json", `{"type":"object","additionalProperties":true}`)
	sections := 10
	write("docs/src/index.json", `{"title":"Индекс","kind":"page"}`)
	for s := 0; s < sections; s++ {
		write(fmt.Sprintf("docs/src/sec-%d.json", s), fmt.Sprintf(`{"title":"Раздел %d","kind":"page","parent":"index"}`, s))
	}
	for i := 0; i < n-1-sections; i++ {
		var b strings.Builder
		paras := 5 + i%12 // about 0.35 KB of Cyrillic text (2 bytes per letter) per paragraph
		fmt.Fprintf(&b, `{"title":"Документ %d","kind":"note","status":"accepted","parent":"sec-%d","tags":["x"],"statement":"Факт номер %d, см. [[doc-%d]].","body":[`, i, i%sections, i, (i+1)%(n-1-sections))
		for k := 0; k < paras; k++ {
			if k > 0 {
				b.WriteString(",")
			}
			text := strings.Repeat(para, 2)
			if k == 0 {
				text += fmt.Sprintf("См. [[doc-%d]].", (i+1)%(n-1-sections))
			}
			fmt.Fprintf(&b, "%q", text)
		}
		b.WriteString(`]}`)
		write(fmt.Sprintf("docs/src/doc-%d.json", i), b.String())
	}
}

// TestScale reports the time of check on 1000 documents by phase.
// Run: DOCC_SCALE=1 go test ./cmd/docc -run Scale -v
func TestScale(t *testing.T) {
	if os.Getenv("DOCC_SCALE") == "" {
		t.Skip("set DOCC_SCALE=1 to measure")
	}
	dir := t.TempDir()
	genScale(t, dir, 1000)
	t.Chdir(dir)
	if code, out, _ := do("build"); code != 0 {
		t.Fatalf("build: %d %s", code, out)
	}
	var total int64
	_ = filepath.WalkDir(filepath.Join(dir, "docs", "src"), func(p string, d os.DirEntry, err error) error {
		if err == nil && !d.IsDir() {
			if st, e := d.Info(); e == nil {
				total += st.Size()
			}
		}
		return nil
	})
	t.Logf("1000 documents, %.1f MB of sources", float64(total)/1e6)
	for run := 1; run <= 3; run++ {
		var t0, t1, t2, t3, t4 time.Time
		t0 = time.Now()
		cfg, _ := config.Load("docc.json", false)
		p, err := engine.Load(cfg)
		if err != nil {
			t.Fatal(err)
		}
		t1 = time.Now()
		p.Validate()
		t2 = time.Now()
		files := (&render.Renderer{P: p}).Files()
		t3 = time.Now()
		render.CheckFresh(p, cfg.Out, files)
		t4 = time.Now()
		t.Logf("run %d: load %v, validate %v, render %v, compare out/ %v, total %v (diagnostics: %d)",
			run, t1.Sub(t0).Round(time.Millisecond), t2.Sub(t1).Round(time.Millisecond), t3.Sub(t2).Round(time.Millisecond),
			t4.Sub(t3).Round(time.Millisecond), t4.Sub(t0).Round(time.Millisecond), len(p.Diags))
	}
	start := time.Now()
	code, _, _ := do("check")
	t.Logf("docc check end to end: %v (exit %d)", time.Since(start).Round(time.Millisecond), code)
}
