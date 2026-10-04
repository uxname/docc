package text

import (
	"reflect"
	"testing"
)

func found(m *Matcher, s string) []string {
	var out []string
	for _, h := range m.Find(s) {
		out = append(out, s[h.Start:h.End])
	}
	return out
}

func TestWords(t *testing.T) {
	var got []string
	for _, w := range Words("По-видимому, it's 3 раза_два; x- -y") {
		got = append(got, w.Text)
	}
	want := []string{"По-видимому", "it's", "3", "раза_два", "x", "y"}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("%q", got)
	}
}

func TestMatcher(t *testing.T) {
	m := NewMatcher([]string{"быстро", "быстр*", "и т.д.", "как правило", "user-friendly", "etc."})
	cases := map[string][]string{
		"Работает БЫСТРО.":              {"БЫСТРО"}, // exact and prefix entries hit the same text
		"быстрота":                      {"быстрота"},
		"небыстро":                      nil,
		"Файлы, ссылки и т.д. и прочее": {"и т.д."},
		"Как  правило, да":              {"Как  правило"},
		"A user-friendly UI, etc.":      {"user-friendly", "etc."},
		"user-friendlyish":              nil,
		"быстро-быстро":                 {"быстро-быстро"}, // prefix entry: the hyphenated word starts with быстр
	}
	for in, want := range cases {
		if got := found(m, in); !reflect.DeepEqual(got, want) {
			t.Errorf("%q: got %q want %q", in, got, want)
		}
	}
}

func TestPrefixAndAllow(t *testing.T) {
	m := NewMatcher([]string{"быстр*"})
	allow := NewMatcher([]string{"быстрорастворимый"})
	s := "быстрорастворимый кофе и быстрая доставка"
	hits := Without(m.Find(s), allow.Find(s))
	if len(hits) != 1 || s[hits[0].Start:hits[0].End] != "быстрая" {
		t.Errorf("%+v", hits)
	}
}

func TestPlaceholders(t *testing.T) {
	m := NewMatcher([]string{"TODO", "TBD", "FIXME", "lorem", "???"})
	if got := found(m, "todo: fix; TBD, Lorem ipsum ??? and todos, xtbd"); !reflect.DeepEqual(got, []string{"todo", "TBD", "Lorem", "???"}) {
		t.Errorf("%q", got)
	}
}

// Lower-casing U+0130 changes the byte length, which forces the slow path;
// both paths must give the same answer.
func TestSlowPathAgrees(t *testing.T) {
	m := NewMatcher([]string{"быстро", "быстр*", "как правило", "etc."})
	for _, s := range []string{"Как  правило, быстро и быстрая; etc.", "Работает БЫСТРО.", "небыстро"} {
		want := found(m, s)
		got := found(m, "İ "+s)
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%q: slow %q fast %q", s, got, want)
		}
	}
}
