// Package config reads docc.json (strict JSON, errors with positions) and
// resolves effective rule levels and parameters.
package config

import (
	"fmt"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/uxname/docc/internal/diag"
	"github.com/uxname/docc/internal/loader"
)

// Error is a configuration error (exit code 2).
type Error struct {
	File      string
	Line, Col int
	Msg       string
}

// Errors are several config errors found in one run, in file order.
type Errors []*Error

func (es Errors) Error() string {
	lines := make([]string, len(es))
	for i, e := range es {
		lines[i] = e.Error()
	}
	return strings.Join(lines, "\n")
}

// Unwrap lets errors.As find the single *Error.
func (es Errors) Unwrap() []error {
	out := make([]error, len(es))
	for i, e := range es {
		out[i] = e
	}
	return out
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

// Branding is the branding section: the name, site, logo and colours of the
// company the HTML build is made for. Nil: the page is unbranded.
type Branding struct {
	Name, URL   string
	Logo        string // path relative to Root; png, jpg, jpeg, gif, svg or webp
	Accent      string // links and marks in the dark theme and on the dark header
	AccentLight string // links and marks on a light page; falls back to Accent
	Header      string // background of the header with the logo
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
	Brand     *Branding

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
		var errs Errors
		for _, d := range r.Dups {
			errs = append(errs, w.errAt(d.Start, "duplicate key %q", d.Key))
		}
		return nil, errs.one()
	}
	c := Default(root)
	c.File = file
	var errs Errors

	// custom rules first: rules refer to their names
	if n := r.Root.Get("customRules"); n != nil {
		if err := w.customRules(c, n); err != nil {
			return nil, err // the other errors may only follow from it
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
		case "branding":
			c.Brand, e = w.branding(m.Value, root)
		default:
			e = w.errAt(m.KeyStart, "unknown key %q", m.Key)
		}
		if e != nil {
			errs = append(errs, e)
		}
	}
	if len(errs) > 0 {
		return nil, errs.one()
	}
	return c, nil
}

// one returns the only error itself, else the list.
func (es Errors) one() error {
	if len(es) == 1 {
		return es[0]
	}
	return es
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

var colorRe = regexp.MustCompile(`^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$`)

var logoExt = map[string]bool{".png": true, ".jpg": true, ".jpeg": true, ".gif": true, ".svg": true, ".webp": true} // as engine.ImageExt

func (w *walker) branding(n *loader.Node, root string) (*Branding, *Error) {
	if n.Kind != loader.Object {
		return nil, w.typeErr(n, "an object")
	}
	b := &Branding{}
	for _, m := range n.Members {
		var e *Error
		switch m.Key {
		case "name":
			b.Name, e = w.str(m.Value)
			if e == nil && strings.TrimSpace(b.Name) == "" {
				e = w.errAt(m.Value.Start, "name must not be empty")
			}
		case "url":
			b.URL, e = w.str(m.Value)
			if u, err := url.Parse(b.URL); e == nil && (err != nil || (u.Scheme != "https" && u.Scheme != "http") || u.Host == "") {
				e = w.errAt(m.Value.Start, "url must be an http or https address")
			}
		case "logo":
			b.Logo, e = w.path(m.Value)
			if e == nil && !logoExt[strings.ToLower(path.Ext(b.Logo))] {
				e = w.errAt(m.Value.Start, "logo must be a png, jpg, jpeg, gif, svg or webp file")
			}
			if e == nil {
				if st, err := os.Stat(filepath.Join(root, filepath.FromSlash(b.Logo))); err != nil || st.IsDir() {
					e = w.errAt(m.Value.Start, "logo file %q does not exist", b.Logo)
				}
			}
		case "accent", "accentLight", "header":
			var v string
			if v, e = w.str(m.Value); e == nil && !colorRe.MatchString(v) {
				e = w.errAt(m.Value.Start, "%s must be a colour like #57e9c7", m.Key)
			}
			switch m.Key {
			case "accent":
				b.Accent = v
			case "accentLight":
				b.AccentLight = v
			default:
				b.Header = v
			}
		default:
			e = w.errAt(m.KeyStart, "unknown key %q", m.Key)
		}
		if e != nil {
			return nil, e
		}
	}
	if b.Name == "" {
		return nil, w.errAt(n.Start, "branding needs a name")
	}
	return b, nil
}
