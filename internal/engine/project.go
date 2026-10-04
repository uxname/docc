// Package engine loads a project (config, schemas, documents) and runs all
// checks, producing diagnostics.
package engine

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/rules"
	"github.com/santiment/docc/internal/schema"
)

var idRe = regexp.MustCompile(`^[a-z0-9]([a-z0-9-]*[a-z0-9])?$`)

var reservedNames = func() map[string]bool {
	m := map[string]bool{"con": true, "prn": true, "aux": true, "nul": true}
	for i := 1; i <= 9; i++ {
		m[fmt.Sprintf("com%d", i)] = true
		m[fmt.Sprintf("lpt%d", i)] = true
	}
	return m
}()

// ValidID reports whether name is a valid document id or folder name.
func ValidID(name string) bool { return idRe.MatchString(name) && !reservedNames[name] }

// Doc is one source file.
type Doc struct {
	ID     string
	Rel    string // path relative to src, with '/'
	File   string // path relative to the project root, with '/'
	Res    *loader.Result
	Root   *loader.Node // nil when the file does not parse
	Hidden bool

	Refs []RefUse
	Incs []IncUse

	strs []StrNode
	done bool
}

// Parsed reports whether the file is valid JSON with an object root.
func (d *Doc) Parsed() bool { return d.Root != nil }

// Project is the loaded project.
type Project struct {
	Cfg     *config.Config
	Schemas *schema.Set
	Docs    []*Doc          // every file with a valid name, ordered by path
	Graph   map[string]*Doc // first document per id
	Diags   []diag.Diagnostic

	byFile map[string]*Doc
}

// UsageError is a problem with the environment (exit code 2).
type UsageError struct{ Msg string }

func (e *UsageError) Error() string { return e.Msg }

// Load reads the config, schemas and all documents, and emits the file-level
// diagnostics (DOC101, DOC102, DOC104).
func Load(cfg *config.Config) (*Project, error) {
	set, err := schema.Load(cfg.Abs(cfg.Schemas), cfg.Schemas)
	if err != nil {
		return nil, err
	}
	p := &Project{Cfg: cfg, Schemas: set, Graph: map[string]*Doc{}, byFile: map[string]*Doc{}}
	srcAbs := cfg.Abs(cfg.Src)
	st, err := os.Stat(srcAbs)
	if err != nil || !st.IsDir() {
		return nil, &UsageError{fmt.Sprintf("source directory %q not found", cfg.Src)}
	}
	var paths []string
	err = filepath.WalkDir(srcAbs, func(abs string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if abs == srcAbs {
			return nil
		}
		name := d.Name()
		if strings.HasPrefix(name, ".") || d.Type()&fs.ModeSymlink != 0 {
			if d.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		rel, _ := filepath.Rel(srcAbs, abs)
		rel = filepath.ToSlash(rel)
		if d.IsDir() {
			if !ValidID(name) {
				p.emitFile(cfg.Src+"/"+rel, "DOC104", 1, 1,
					fmt.Sprintf("folder name %q is not a valid id", name),
					"Use only a-z, 0-9 and '-' in folder names, not starting or ending with '-', and avoid reserved Windows names.")
			}
			return nil
		}
		if strings.HasSuffix(name, ".json") && d.Type().IsRegular() {
			paths = append(paths, rel)
		}
		return nil
	})
	if err != nil {
		return nil, &UsageError{err.Error()}
	}
	sort.Strings(paths)
	for _, rel := range paths {
		id := strings.TrimSuffix(path1(rel), ".json")
		file := cfg.Src + "/" + rel
		if !ValidID(id) {
			p.emitFile(file, "DOC104", 1, 1,
				fmt.Sprintf("file name %q is not a valid id", id),
				"Rename the file: use only a-z, 0-9 and '-', not starting or ending with '-', and avoid reserved Windows names.")
			continue
		}
		d := &Doc{ID: id, Rel: rel, File: file}
		p.read(d, srcAbs)
		p.Docs = append(p.Docs, d)
		p.byFile[file] = d
	}
	for _, d := range p.Docs {
		if _, taken := p.Graph[d.ID]; !taken {
			p.Graph[d.ID] = d
		}
	}
	return p, nil
}

func path1(rel string) string {
	if i := strings.LastIndexByte(rel, '/'); i >= 0 {
		return rel[i+1:]
	}
	return rel
}

func (p *Project) read(d *Doc, srcAbs string) {
	abs := filepath.Join(srcAbs, filepath.FromSlash(d.Rel))
	var data []byte
	if st, err := os.Stat(abs); err == nil && st.Size() > loader.MaxFileSize {
		d.Res = &loader.Result{Src: &loader.Source{}, Err: &loader.Error{Msg: "file is larger than 5 MB"}}
	} else if data, err = os.ReadFile(abs); err != nil {
		d.Res = &loader.Result{Src: &loader.Source{}, Err: &loader.Error{Msg: "cannot read file: " + err.Error()}}
	} else {
		d.Res = loader.Parse(data)
	}
	if d.Res.Err != nil {
		hint := "Fix the JSON syntax: strict JSON only, no comments or trailing commas."
		if strings.Contains(string(data), "<<<<<<<") || strings.Contains(string(data), ">>>>>>>") {
			hint = "Resolve the merge conflict markers in the file first."
		}
		p.emit(d, "DOC101", d.Res.Err.Offset, d.Res.Err.Offset, "invalid JSON: "+d.Res.Err.Msg, hint)
		return
	}
	d.Root = d.Res.Root
	for _, dup := range d.Res.Dups {
		p.emit(d, "DOC102", dup.Start, dup.End, fmt.Sprintf("duplicate key %q", dup.Key),
			"Remove one of the repeated keys or merge their values.")
	}
	if h := d.Root.Get("hidden"); h != nil && h.Kind == loader.Bool && h.Str == "true" {
		d.Hidden = true
	}
}

func (p *Project) newDiag(file, code string) (*diag.Diagnostic, *rules.Rule) {
	r := rules.ByCode(code)
	return &diag.Diagnostic{Code: code, Rule: r.Name, File: file, Fixable: r.Fix}, r
}

func (p *Project) push(dg *diag.Diagnostic, lvl diag.Level) *diag.Diagnostic {
	dg.Lvl = lvl
	dg.Level = lvl.String()
	p.Diags = append(p.Diags, *dg)
	return &p.Diags[len(p.Diags)-1]
}

// emit adds a diagnostic on a document, at the byte range [start,end).
// Returns nil when the rule is off for the document; the pointer is valid
// only until the next emit.
func (p *Project) emit(d *Doc, code string, start, end int, msg, hint string) *diag.Diagnostic {
	lvl := p.Cfg.For(d.Rel).Rule(code).Level
	if lvl == diag.Off {
		return nil
	}
	dg, _ := p.newDiag(d.File, code)
	dg.DocID = d.ID
	dg.Message, dg.Hint = msg, hint
	dg.Start, dg.End = start, end
	dg.Line, dg.Column = d.Res.Src.Position(start)
	dg.EndLine, dg.EndColumn = d.Res.Src.Position(end)
	return p.push(dg, lvl)
}

// emitNoPos adds a document diagnostic that has no place in the JSON text.
func (p *Project) emitNoPos(d *Doc, code, msg, hint string) *diag.Diagnostic {
	lvl := p.Cfg.For(d.Rel).Rule(code).Level
	if lvl == diag.Off {
		return nil
	}
	dg, _ := p.newDiag(d.File, code)
	dg.DocID = d.ID
	dg.Message, dg.Hint = msg, hint
	dg.Line, dg.Column, dg.EndLine, dg.EndColumn = 1, 1, 1, 1
	dg.NoPos = true
	return p.push(dg, lvl)
}

// emitFile adds a diagnostic on a non-document file (config, folder, code, out).
func (p *Project) emitFile(file, code string, line, col int, msg, hint string) *diag.Diagnostic {
	lvl := p.Cfg.For("").Rule(code).Level
	if lvl == diag.Off {
		return nil
	}
	dg, _ := p.newDiag(file, code)
	dg.Message, dg.Hint = msg, hint
	dg.Line, dg.Column, dg.EndLine, dg.EndColumn = line, col, line, col
	dg.NoPos = true
	return p.push(dg, lvl)
}

// DocByFile returns the document for a project-relative file path.
func (p *Project) DocByFile(file string) *Doc { return p.byFile[file] }

// Off reports whether a rule is disabled for every file, so a whole pass can
// be skipped. With overrides present it is conservative and returns false.
func (p *Project) Off(code string) bool {
	return len(p.Cfg.Overrides) == 0 && p.Cfg.For("").Rule(code).Level == diag.Off
}
