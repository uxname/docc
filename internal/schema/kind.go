package schema

import (
	"errors"
	"fmt"
	"maps"
	"net/url"
	"slices"
	"strings"

	"github.com/santhosh-tekuri/jsonschema/v6"

	"github.com/santiment/docc/internal/loader"
)

func newKind(sc *jsonschema.Schema, root *loader.Node, u string, l memLoader) (*Kind, error) {
	k := &Kind{schema: sc, closed: root.Get("additionalProperties").Str == "false", allowed: map[string]bool{}, targets: map[string][]string{}}
	if c := root.Get("x-docc-covered"); c != nil {
		if c.Kind != loader.Bool {
			return nil, errors.New(`"x-docc-covered" must be true or false`)
		}
		k.Covered = c.Str == "true"
	}
	return k, k.walk(l[u], u, l, map[string]bool{u + "#": true})
}

// walk collects the top-level names a kind allows and their x-docc-target:
// "properties" of the schema, of its "allOf" branches and of local "$ref"
// targets, recursively. Properties under if/then are not counted.
func (k *Kind) walk(v any, base string, l memLoader, seen map[string]bool) error {
	m, _ := v.(map[string]any)
	props, _ := m["properties"].(map[string]any)
	for _, name := range slices.Sorted(maps.Keys(props)) {
		k.allowed[name] = true
		pm, _ := props[name].(map[string]any)
		t, ok := pm["x-docc-target"]
		if !ok {
			continue
		}
		list, _ := t.([]any)
		if len(list) == 0 {
			return fmt.Errorf(`"x-docc-target" of property %q must be a non-empty array of kind names`, name)
		}
		for _, x := range list {
			s, ok := x.(string)
			if !ok {
				return fmt.Errorf(`"x-docc-target" of property %q must be a non-empty array of kind names`, name)
			}
			if !slices.Contains(k.targets[name], s) {
				k.targets[name] = append(k.targets[name], s)
			}
		}
	}
	if ref, ok := m["$ref"].(string); ok {
		if u, frag, err := splitURL(base, ref); err == nil && !seen[u+"#"+frag] {
			seen[u+"#"+frag] = true
			if t, ok := pointerIn(l[u], frag); ok {
				if err := k.walk(t, u, l, seen); err != nil {
					return err
				}
			}
		}
	}
	branches, _ := m["allOf"].([]any)
	for _, b := range branches {
		if err := k.walk(b, base, l, seen); err != nil {
			return err
		}
	}
	return nil
}

func splitURL(base, ref string) (string, string, error) {
	b, err := url.Parse(base)
	if err != nil {
		return "", "", err
	}
	r, err := b.Parse(ref)
	if err != nil {
		return "", "", err
	}
	frag := r.Fragment
	r.Fragment = ""
	return r.String(), frag, nil
}

// pointerIn resolves a JSON pointer fragment ("" or "/a/b") in a raw value.
func pointerIn(v any, frag string) (any, bool) {
	if v == nil || (frag != "" && !strings.HasPrefix(frag, "/")) {
		return nil, false
	}
	if frag == "" {
		return v, true
	}
	for _, t := range strings.Split(frag[1:], "/") {
		t = strings.ReplaceAll(strings.ReplaceAll(t, "~1", "/"), "~0", "~")
		m, ok := v.(map[string]any)
		if !ok {
			return nil, false
		}
		if v, ok = m[t]; !ok {
			return nil, false
		}
	}
	return v, true
}

// Kind returns the kind schema, or nil when there is none.
func (s *Set) Kind(name string) *Kind { return s.kinds[name] }

// Targets returns the kinds that references in the field must point to
// ("x-docc-target"); nil means any kind.
func (k *Kind) Targets(field string) []string { return k.targets[field] }

// unknownFields reports the fields of a closed kind that the schema does not
// allow; the hint names the closest allowed name.
func (k *Kind) unknownFields(root *loader.Node) []Violation {
	if !k.closed {
		return nil
	}
	var out []Violation
	for _, m := range root.Members {
		if k.allowed[m.Key] || slices.Contains(builtinFields, m.Key) || m.Key == "docIgnore" {
			continue
		}
		hint := fmt.Sprintf("Remove the field %q or add it to \"properties\" of the kind schema.", m.Key)
		if c := k.closest(m.Key); c != "" {
			hint = fmt.Sprintf("Did you mean %q? ", c) + hint
		}
		out = append(out, Violation{Pointer: Pointer([]string{m.Key}), Message: fmt.Sprintf("unknown field %q", m.Key), Hint: hint, Start: m.KeyStart, End: m.KeyEnd})
	}
	return out
}

func (k *Kind) closest(key string) string {
	names := append(slices.Sorted(maps.Keys(k.allowed)), builtinFields...)
	best, bestD := "", 0
	for _, n := range names {
		if d := distance(key, n); best == "" || d < bestD {
			best, bestD = n, d
		}
	}
	if bestD > 2 && bestD*3 > len([]rune(key)) {
		return ""
	}
	return best
}

// distance is the Levenshtein distance between a and b.
func distance(a, b string) int {
	x, y := []rune(a), []rune(b)
	prev := make([]int, len(y)+1)
	for j := range prev {
		prev[j] = j
	}
	for i := 1; i <= len(x); i++ {
		cur := make([]int, len(y)+1)
		cur[0] = i
		for j := 1; j <= len(y); j++ {
			cost := 1
			if x[i-1] == y[j-1] {
				cost = 0
			}
			cur[j] = min(prev[j]+1, cur[j-1]+1, prev[j-1]+cost)
		}
		prev = cur
	}
	return prev[len(y)]
}
