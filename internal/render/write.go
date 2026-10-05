package render

import (
	"bytes"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/santiment/docc/internal/engine"
)

// inside reports whether path a is b or lies under b.
func inside(a, b string) bool {
	a, b = filepath.Clean(a), filepath.Clean(b)
	return a == b || strings.HasPrefix(a, b+string(filepath.Separator))
}

// CheckDir refuses an output directory that docc must not empty: one that
// holds the project root, the config, the sources or the schemas, lies inside
// the sources or the schemas, or overlaps the other output directory.
func CheckDir(p *engine.Project, rel string) error {
	cfg := p.Cfg
	out := cfg.Abs(rel)
	src, sch := cfg.Abs(cfg.Src), cfg.Abs(cfg.Schemas)
	bad := inside(cfg.Root, out) || inside(src, out) || inside(sch, out) ||
		inside(filepath.Join(cfg.Root, filepath.Base(cfg.File)), out) ||
		inside(out, src) || inside(out, sch)
	if filepath.Clean(cfg.Abs(cfg.Out)) == filepath.Clean(cfg.Abs(cfg.OutFull)) {
		bad = true
	}
	for _, other := range []string{cfg.Out, cfg.OutFull} {
		if other != rel && (inside(out, cfg.Abs(other)) || inside(cfg.Abs(other), out)) {
			bad = true
		}
	}
	if bad {
		return &engine.UsageError{Msg: fmt.Sprintf("output directory %q overlaps the project (root, config, sources, schemas or the other output directory); choose a dedicated directory", rel)}
	}
	return nil
}

// SingleFile returns the absolute path of a single-file build: the output
// directory with ext added ("docs/out" gives "docs/out.md"). It refuses a path
// that would overwrite the config, the sources or the schemas.
func SingleFile(p *engine.Project, outRel, ext string) (string, error) {
	cfg := p.Cfg
	f := cfg.Abs(outRel) + ext
	if inside(f, cfg.Abs(cfg.Src)) || inside(f, cfg.Abs(cfg.Schemas)) ||
		filepath.Clean(f) == filepath.Join(cfg.Root, filepath.Base(cfg.File)) {
		return "", &engine.UsageError{Msg: fmt.Sprintf("single file %q overlaps the project (config, sources or schemas); choose another output directory", f)}
	}
	return f, nil
}

// WriteFile writes one file (LF, UTF-8) unless it already has this content.
func WriteFile(path, content string) error {
	if old, err := os.ReadFile(path); err == nil && bytes.Equal(old, []byte(content)) {
		return nil
	}
	return os.WriteFile(path, []byte(content), 0o644)
}

// Write makes dir contain exactly files: writes them (LF, UTF-8, no BOM) and
// removes everything else, including foreign files and folders.
func Write(dir string, files map[string]string) error {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	for name, content := range files {
		p := filepath.Join(dir, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			return err
		}
		if old, err := os.ReadFile(p); err == nil && bytes.Equal(old, []byte(content)) {
			continue
		}
		if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
			return err
		}
	}
	// remove foreign entries, deepest first
	var extras []string
	err := filepath.WalkDir(dir, func(p string, d fs.DirEntry, err error) error {
		if err != nil || p == dir {
			return err
		}
		rel, _ := filepath.Rel(dir, p)
		rel = filepath.ToSlash(rel)
		if d.IsDir() {
			extras = append(extras, p) // dirs are removed below when empty or foreign
			return nil
		}
		if _, ok := files[rel]; !ok {
			extras = append(extras, p)
		}
		return nil
	})
	if err != nil {
		return err
	}
	sort.Slice(extras, func(i, j int) bool { return len(extras[i]) > len(extras[j]) })
	for _, p := range extras {
		st, err := os.Lstat(p)
		if err != nil {
			continue
		}
		if st.IsDir() {
			if entries, _ := os.ReadDir(p); len(entries) == 0 {
				if err := os.Remove(p); err != nil {
					return err
				}
			}
			continue
		}
		if err := os.Remove(p); err != nil {
			return err
		}
	}
	return nil
}

// normalize strips a BOM and turns CRLF into LF, so files edited on Windows
// compare equal.
func normalize(b []byte) []byte {
	b = bytes.TrimPrefix(b, []byte{0xEF, 0xBB, 0xBF})
	return bytes.ReplaceAll(b, []byte("\r\n"), []byte("\n"))
}

// src names the source document of a generated page, if there is one.
func src(p *engine.Project, rel string) string {
	if id := strings.TrimSuffix(rel, ".md"); id != rel && p.Graph[id] != nil {
		return " (source: " + p.Graph[id].File + ")"
	}
	return ""
}

// CheckFresh compares dir with the expected files and reports DOC501 for
// missing, outdated and extra files.
func CheckFresh(p *engine.Project, outRel string, files map[string]string) {
	dir := p.Cfg.Abs(outRel)
	seen := map[string]bool{}
	_ = filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() {
			return nil
		}
		rel, _ := filepath.Rel(dir, path)
		rel = filepath.ToSlash(rel)
		seen[rel] = true
		want, ok := files[rel]
		file := outRel + "/" + rel
		if !ok {
			p.ReportFile(file, "DOC501", 1, 1, "extra file in the output directory (no source produces it)", "Run docc build: it removes files that no source produces.")
			return nil
		}
		got, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(normalize(got), []byte(want)) {
			p.ReportFile(file, "DOC501", 1, 1, "the generated file is outdated"+src(p, rel), "Run docc build and commit the result.")
		}
		return nil
	})
	var missing []string
	for rel := range files {
		if !seen[rel] {
			missing = append(missing, rel)
		}
	}
	sort.Strings(missing)
	for _, rel := range missing {
		p.ReportFile(outRel+"/"+rel, "DOC501", 1, 1, "the generated file is missing"+src(p, rel), "Run docc build and commit the result.")
	}
}
