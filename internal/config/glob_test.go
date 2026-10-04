package config

import "testing"

func TestGlob(t *testing.T) {
	cases := []struct {
		mask, path string
		want       bool
	}{
		{"drafts/**", "drafts/a.json", true},
		{"drafts/**", "drafts/x/y.json", true},
		{"drafts/*", "drafts/x/y.json", false},
		{"*.json", "a.json", true},
		{"*.json", "d/a.json", false},
		{"**/a.json", "a.json", true},
		{"**/a.json", "x/y/a.json", true},
		{"a.json", "ab.json", false},
		{"tests/**", "tests/unit/a_test.go", true},
	}
	for _, c := range cases {
		g, err := CompileGlob(c.mask)
		if err != nil {
			t.Fatal(err)
		}
		if got := g.Match(c.path); got != c.want {
			t.Errorf("%q vs %q: got %v", c.mask, c.path, got)
		}
	}
}
