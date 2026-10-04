// Package schema validates documents with JSON Schema (draft 2020-12): the
// built-in document schema and per-kind schemas from the schemas directory.
// Validation errors are mapped back to positions in the source text.
package schema

import (
	_ "embed"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/santhosh-tekuri/jsonschema/v6"
	"github.com/santhosh-tekuri/jsonschema/v6/kind"
	"golang.org/x/text/language"
	"golang.org/x/text/message"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/loader"
)

//go:embed document.json
var DocumentSchema []byte

const (
	builtinURL = "docc://builtin/document.json"
	schemasURL = "docc://s/schemas/"
)

// Violation is one schema error with the place it points to.
type Violation struct {
	Pointer    string
	Message    string
	Hint       string
	Start, End int
}

// Set holds the compiled built-in schema and the kind schemas.
type Set struct {
	builtin *jsonschema.Schema
	kinds   map[string]*jsonschema.Schema
}

// memLoader serves only preloaded resources: no network, no files outside
// the schemas directory.
type memLoader map[string]any

func (m memLoader) Load(u string) (any, error) {
	if v, ok := m[u]; ok {
		return v, nil
	}
	return nil, fmt.Errorf("reference %q is outside the schemas directory or remote (not allowed)", u)
}

func newCompiler(l memLoader) *jsonschema.Compiler {
	c := jsonschema.NewCompiler()
	c.DefaultDraft(jsonschema.Draft2020)
	c.AssertFormat()
	c.UseLoader(l)
	return c
}

// Load compiles the built-in schema and every *.json file under dir.
// A missing dir is fine. displayDir is the project-relative path for messages.
func Load(dir, displayDir string) (*Set, error) {
	res := loader.Parse(DocumentSchema)
	if res.Err != nil {
		return nil, fmt.Errorf("built-in schema: %s", res.Err.Msg)
	}
	l := memLoader{builtinURL: loader.Value(res.Root)}
	files := map[string]string{} // rel path -> url
	if st, err := os.Stat(dir); err == nil && st.IsDir() {
		var rels []string
		err := filepath.WalkDir(dir, func(p string, d fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if !d.IsDir() && strings.HasSuffix(d.Name(), ".json") && d.Type().IsRegular() {
				rel, _ := filepath.Rel(dir, p)
				rels = append(rels, filepath.ToSlash(rel))
			}
			return nil
		})
		if err != nil {
			return nil, &config.Error{File: displayDir, Msg: err.Error()}
		}
		sort.Strings(rels)
		for _, rel := range rels {
			shown := displayDir + "/" + rel
			data, err := os.ReadFile(filepath.Join(dir, filepath.FromSlash(rel)))
			if err != nil {
				return nil, &config.Error{File: shown, Msg: err.Error()}
			}
			r := loader.Parse(data)
			if r.Err != nil {
				ln, col := r.Src.Position(r.Err.Offset)
				return nil, &config.Error{File: shown, Line: ln, Col: col, Msg: "invalid schema file: " + r.Err.Msg}
			}
			if len(r.Dups) > 0 {
				ln, col := r.Src.Position(r.Dups[0].Start)
				return nil, &config.Error{File: shown, Line: ln, Col: col, Msg: fmt.Sprintf("duplicate key %q", r.Dups[0].Key)}
			}
			u := schemasURL + rel
			l[u] = loader.Value(r.Root)
			files[rel] = u
		}
	}
	c := newCompiler(l)
	s := &Set{kinds: map[string]*jsonschema.Schema{}}
	var err error
	if s.builtin, err = c.Compile(builtinURL); err != nil {
		return nil, fmt.Errorf("built-in schema: %v", err)
	}
	rels := make([]string, 0, len(files))
	for rel := range files {
		rels = append(rels, rel)
	}
	sort.Strings(rels)
	for _, rel := range rels {
		sc, err := c.Compile(files[rel])
		if err != nil {
			return nil, &config.Error{File: displayDir + "/" + rel, Msg: "cannot compile schema: " + oneLine(err)}
		}
		if !strings.Contains(rel, "/") {
			s.kinds[strings.TrimSuffix(rel, ".json")] = sc
		}
	}
	return s, nil
}

func oneLine(err error) string {
	return strings.Join(strings.Fields(err.Error()), " ")
}

// HasKind reports whether a schema file exists for the kind.
func (s *Set) HasKind(kindName string) bool { return s.kinds[kindName] != nil }

// builtinFields are the fields the built-in schema is applied to (DOC105).
var builtinFields = []string{"title", "kind", "status", "parent", "links", "tags", "reviewed", "body", "notes", "hidden"}

// CheckBuiltin validates the built-in fields of a document (DOC105).
// A missing or blank title is left to DOC103 and is not reported here.
func (s *Set) CheckBuiltin(root *loader.Node) []Violation {
	inst := map[string]any{}
	for _, f := range builtinFields {
		if n := root.Get(f); n != nil {
			inst[f] = loader.Value(n)
		}
	}
	vs := s.run(s.builtin, inst, root)
	out := vs[:0]
	for _, v := range vs {
		if v.Pointer == "" && strings.HasPrefix(v.Message, "missing property 'title'") {
			continue
		}
		if v.Pointer == "/title" && !strings.Contains(v.Message, "got ") {
			continue // blank title: DOC103
		}
		out = append(out, v)
	}
	return out
}

// CheckKind validates a whole document against the schema of its kind
// (DOC106). Service fields are removed first.
func (s *Set) CheckKind(kindName string, root *loader.Node) []Violation {
	sc := s.kinds[kindName]
	if sc == nil {
		return nil
	}
	inst := map[string]any{}
	for _, m := range root.Members {
		switch m.Key {
		case "$schema", "docc-ignore", "reviewed", "notes", "hidden":
			continue
		}
		if _, ok := inst[m.Key]; !ok {
			inst[m.Key] = loader.Value(m.Value)
		}
	}
	return s.run(sc, inst, root)
}

var printer = message.NewPrinter(language.English)

func (s *Set) run(sc *jsonschema.Schema, inst any, root *loader.Node) []Violation {
	err := sc.Validate(inst)
	if err == nil {
		return nil
	}
	ve, ok := err.(*jsonschema.ValidationError)
	if !ok {
		return []Violation{{Message: err.Error(), Start: root.Start, End: root.Start + 1}}
	}
	var out []Violation
	collect(ve, root, &out)
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].Start != out[j].Start {
			return out[i].Start < out[j].Start
		}
		return out[i].Message < out[j].Message
	})
	// drop exact duplicates
	uniq := out[:0]
	for i, v := range out {
		if i > 0 && v.Start == out[i-1].Start && v.Message == out[i-1].Message {
			continue
		}
		uniq = append(uniq, v)
	}
	return uniq
}

func collect(ve *jsonschema.ValidationError, root *loader.Node, out *[]Violation) {
	switch ve.ErrorKind.(type) {
	case *kind.OneOf, *kind.AnyOf:
		n := nodeAt(root, ve.InstanceLocation)
		*out = append(*out, mk(ve.InstanceLocation, "value does not match any allowed shape", "Use one of the shapes the schema allows for this field.", n.Start, n.End))
		return
	}
	if len(ve.Causes) > 0 {
		for _, c := range ve.Causes {
			collect(c, root, out)
		}
		return
	}
	loc := ve.InstanceLocation
	n := nodeAt(root, loc)
	text := ve.ErrorKind.LocalizedString(printer)
	switch k := ve.ErrorKind.(type) {
	case *kind.Required:
		for _, m := range k.Missing {
			*out = append(*out, mk(loc, fmt.Sprintf("missing property '%s'", m), fmt.Sprintf("Add the field %q.", m), n.Start, n.Start+1))
		}
	case *kind.AdditionalProperties:
		for _, p := range k.Properties {
			start, end := n.Start, n.Start+1
			for _, m := range n.Members {
				if m.Key == p {
					start, end = m.KeyStart, m.KeyEnd
					break
				}
			}
			*out = append(*out, mk(loc, fmt.Sprintf("property '%s' is not allowed", p), fmt.Sprintf("Remove the field %q or allow it in the schema.", p), start, end))
		}
	case *kind.UniqueItems:
		idx := k.Duplicates[1]
		if k.Duplicates[0] > idx {
			idx = k.Duplicates[0]
		}
		item := nodeAt(root, append(append([]string{}, loc...), fmt.Sprint(idx)))
		*out = append(*out, mk(append(append([]string{}, loc...), fmt.Sprint(idx)), text, "Remove the repeated element.", item.Start, item.End))
	case *kind.Type:
		*out = append(*out, mk(loc, text, "Use a value of the required type.", n.Start, n.End))
	case *kind.Enum:
		*out = append(*out, mk(loc, text, "Use one of the allowed values.", n.Start, n.End))
	default:
		*out = append(*out, mk(loc, text, "Change the value so that it matches the schema.", n.Start, n.End))
	}
}

func mk(loc []string, msg, hint string, start, end int) Violation {
	p := Pointer(loc)
	m := msg
	if p != "" {
		m = p + ": " + msg
	}
	return Violation{Pointer: p, Message: m, Hint: hint, Start: start, End: end}
}

// Pointer renders tokens as a JSON pointer.
func Pointer(tokens []string) string {
	var b strings.Builder
	for _, t := range tokens {
		b.WriteByte('/')
		t = strings.ReplaceAll(t, "~", "~0")
		b.WriteString(strings.ReplaceAll(t, "/", "~1"))
	}
	return b.String()
}

// nodeAt walks the loader tree by pointer tokens; falls back to the deepest
// node reached.
func nodeAt(n *loader.Node, tokens []string) *loader.Node {
	for _, t := range tokens {
		var next *loader.Node
		switch n.Kind {
		case loader.Object:
			next = n.Get(t)
		case loader.Array:
			var i int
			if _, err := fmt.Sscanf(t, "%d", &i); err == nil && i >= 0 && i < len(n.Items) {
				next = n.Items[i]
			}
		}
		if next == nil {
			return n
		}
		n = next
	}
	return n
}

// ResolvePointer resolves an RFC 6901 pointer string ("" is the root) on a
// document. Returns nil when the pointer does not exist.
func ResolvePointer(root *loader.Node, ptr string) *loader.Node {
	if ptr == "" {
		return root
	}
	if !strings.HasPrefix(ptr, "/") {
		return nil
	}
	n := root
	for _, t := range strings.Split(ptr[1:], "/") {
		t = strings.ReplaceAll(strings.ReplaceAll(t, "~1", "/"), "~0", "~")
		var next *loader.Node
		switch n.Kind {
		case loader.Object:
			next = n.Get(t)
		case loader.Array:
			var i int
			if _, err := fmt.Sscanf(t, "%d", &i); err == nil && fmt.Sprint(i) == t && i >= 0 && i < len(n.Items) {
				next = n.Items[i]
			}
		}
		if next == nil {
			return nil
		}
		n = next
	}
	return n
}
