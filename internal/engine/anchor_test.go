package engine

import "testing"

func TestSplitAnchor(t *testing.T) {
	cases := []struct{ in, text, name string }{
		{"Limit is 5. ^limit", "Limit is 5.", "limit"},
		{"Release ^new", "Release", "new"},
		{"line one\nline two ^end", "line one\nline two", "end"},
		{"x ^2", "x ^2", ""},
		{"react ^18", "react ^18", ""},
		{"a note[^1]", "a note[^1]", ""},
		{"see ^1a", "see ^1a", ""},
		{"Upper ^Name", "Upper ^Name", ""},
		{"^solo", "^solo", ""},
		{"middle ^a\nnext line", "middle ^a\nnext line", ""},
		{"trailing ^a ", "trailing ^a ", ""},
	}
	for _, c := range cases {
		if text, name := SplitAnchor(c.in); text != c.text || name != c.name {
			t.Errorf("%q: got %q %q, want %q %q", c.in, text, name, c.text, c.name)
		}
	}
}
