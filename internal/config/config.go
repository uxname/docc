// Package config reads docc.json (strict JSON, errors with positions) and
// resolves effective rule levels and parameters.
package config

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/loader"
)

// Error is a configuration error (exit code 2).
type Error struct {
	File      string
	Line, Col int
	Msg       string
}

func (e *Error) Error() string {
	if e.Line == 0 {
		return fmt.Sprintf("%s: config error: %s", e.File, e.Msg)
	}
	return fmt.Sprintf("%s:%d:%d: config error: %s", e.File, e.Line, e.Col, e.Msg)
}

type Params struct {
	Words, Allow []string
}

// Entry is one value of the rules section: a level and/or parameters.
type Entry struct {
	Level  *diag.Level // nil: level from the preset
	Params Params
}

// RuleSet is the content of the rules section.
// ByRule is keyed by rule code, or by name for custom rules (DOC800 for all custom).
type RuleSet struct {
	ByRule  map[string]*Entry
	ByGroup map[string]diag.Level
}

type Custom struct {
	ID, Message, Hint, Kind, Given string
	Level                          diag.Level
	Forbid, Require                *regexp.Regexp
}

type Config struct {
	Root     string // absolute project root
	File     string // display name of the config file
	Src      string // paths relative to Root, with '/'
	Out      string
	Schemas  string
	OutFull  string
	Roots    []string
	RootsPos [2]int // line, column of the roots value (0: default)

	Tags    []string
	HasTags bool

	Recommended   bool
	Numbering     bool
	Lang          string // "en" or "ru": language of the fixed labels in the output
	MaxWarnings   int
	HasMaxWarning bool

	Rules     RuleSet
	Custom    []*Custom
	TraceScan []*Glob

	resolved *Resolved
}

func Default(root string) *Config {
	return &Config{
		Root: root, File: "docc.json",
		Src: "docs/src", Out: "docs/out", Schemas: "docs/schemas", OutFull: "docs/out-full",
		Roots: []string{"index"}, Recommended: true, Lang: "en",
		Rules: RuleSet{ByRule: map[string]*Entry{}, ByGroup: map[string]diag.Level{}},
	}
}

// Abs returns the OS path for a project-relative '/' path.
func (c *Config) Abs(rel string) string { return filepath.Join(c.Root, filepath.FromSlash(rel)) }

// Load reads the config at file. If explicit is false and the file does not
// exist, defaults are returned. The project root is the file's directory.
func Load(file string, explicit bool) (*Config, error) {
	abs, err := filepath.Abs(file)
	if err != nil {
		return nil, err
	}
	root := filepath.Dir(abs)
	data, err := os.ReadFile(abs)
	if err != nil {
		if os.IsNotExist(err) && !explicit {
			return Default(root), nil
		}
		return nil, &Error{File: file, Msg: err.Error()}
	}
	return Parse(data, root, filepath.ToSlash(file))
}

type walker struct {
	src  *loader.Source
	file string
}

func (w *walker) errAt(off int, format string, a ...any) *Error {
	l, c := w.src.Position(off)
	return &Error{File: w.file, Line: l, Col: c, Msg: fmt.Sprintf(format, a...)}
}

func (w *walker) typeErr(n *loader.Node, want string) *Error {
	return w.errAt(n.Start, "expected %s", want)
}

func (w *walker) str(n *loader.Node) (string, *Error) {
	if n.Kind != loader.String {
		return "", w.typeErr(n, "a string")
	}
	return n.Str, nil
}

func (w *walker) strs(n *loader.Node) ([]string, *Error) {
	if n.Kind != loader.Array {
		return nil, w.typeErr(n, "an array of strings")
	}
	out := make([]string, 0, len(n.Items))
	for _, it := range n.Items {
		s, e := w.str(it)
		if e != nil {
			return nil, e
		}
		out = append(out, s)
	}
	return out, nil
}

func (w *walker) path(n *loader.Node) (string, *Error) {
	s, e := w.str(n)
	if e != nil {
		return "", e
	}
	s = path.Clean(filepath.ToSlash(s))
	if s == "." || path.IsAbs(s) || strings.HasPrefix(s, "../") || s == ".." {
		return "", w.errAt(n.Start, "path must be relative to the project root and inside it")
	}
	return s, nil
}

// Parse parses docc.json content. root is the project root.
func Parse(data []byte, root, file string) (*Config, error) {
	r := loader.Parse(data)
	w := &walker{src: r.Src, file: file}
	if r.Err != nil {
		return nil, w.errAt(r.Err.Offset, "%s", r.Err.Msg)
	}
	if len(r.Dups) > 0 {
		return nil, w.errAt(r.Dups[0].Start, "duplicate key %q", r.Dups[0].Key)
	}
	c := Default(root)
	c.File = file

	// custom rules first: rules refer to their names
	if n := r.Root.Get("customRules"); n != nil {
		if err := w.customRules(c, n); err != nil {
			return nil, err
		}
	}
	for _, m := range r.Root.Members {
		var e *Error
		switch m.Key {
		case "customRules":
		case "src":
			c.Src, e = w.path(m.Value)
		case "out":
			c.Out, e = w.path(m.Value)
		case "schemas":
			c.Schemas, e = w.path(m.Value)
		case "outFull":
			c.OutFull, e = w.path(m.Value)
		case "roots":
			c.Roots, e = w.strs(m.Value)
			if e == nil && len(c.Roots) == 0 {
				e = w.errAt(m.Value.Start, "roots must not be empty")
			}
			c.RootsPos[0], c.RootsPos[1] = r.Src.Position(m.Value.Start)
		case "tags":
			c.Tags, e = w.strs(m.Value)
			c.HasTags = true
		case "recommended":
			if m.Value.Kind != loader.Bool {
				e = w.typeErr(m.Value, "true or false")
			} else {
				c.Recommended = m.Value.Str == "true"
			}
		case "numbering":
			if m.Value.Kind != loader.Bool {
				e = w.typeErr(m.Value, "true or false")
			} else {
				c.Numbering = m.Value.Str == "true"
			}
		case "lang":
			c.Lang, e = w.str(m.Value)
			if e == nil && c.Lang != "en" && c.Lang != "ru" {
				e = w.errAt(m.Value.Start, "lang must be \"en\" or \"ru\"")
			}
		case "maxWarnings":
			c.MaxWarnings, e = w.nonNegInt(m.Value)
			c.HasMaxWarning = e == nil
		case "rules":
			c.Rules, e = w.ruleSet(c, m.Value)
		case "trace":
			e = w.trace(c, m.Value)
		default:
			e = w.errAt(m.KeyStart, "unknown key %q", m.Key)
		}
		if e != nil {
			return nil, e
		}
	}
	return c, nil
}

func (w *walker) nonNegInt(n *loader.Node) (int, *Error) {
	var v int
	if n.Kind != loader.Number {
		return 0, w.typeErr(n, "a non-negative integer")
	}
	if _, err := fmt.Sscanf(n.Str, "%d", &v); err != nil || v < 0 || strings.ContainsAny(n.Str, ".eE") {
		return 0, w.typeErr(n, "a non-negative integer")
	}
	return v, nil
}

func (w *walker) trace(c *Config, n *loader.Node) *Error {
	if n.Kind != loader.Object {
		return w.typeErr(n, "an object")
	}
	for _, m := range n.Members {
		if m.Key != "scan" {
			return w.errAt(m.KeyStart, "unknown key %q", m.Key)
		}
		masks, e := w.strs(m.Value)
		if e != nil {
			return e
		}
		for _, mk := range masks {
			g, err := CompileGlob(mk)
			if err != nil {
				return w.errAt(m.Value.Start, "bad mask %q: %v", mk, err)
			}
			c.TraceScan = append(c.TraceScan, g)
		}
	}
	return nil
}
