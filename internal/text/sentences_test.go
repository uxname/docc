package text

import (
	"reflect"
	"testing"
)

func sents(s string) []string {
	var out []string
	for _, sp := range Sentences(s) {
		out = append(out, s[sp.Start:sp.End])
	}
	return out
}

func TestSentences(t *testing.T) {
	cases := map[string][]string{
		"Первое. Второе! Третье?":                  {"Первое.", "Второе!", "Третье?"},
		"Файлы, ссылки и т.д. Но дальше идёт ещё.": {"Файлы, ссылки и т.д. Но дальше идёт ещё."},
		"Версия 3.14 вышла. Она хороша.":           {"Версия 3.14 вышла.", "Она хороша."},
		"См. рис. 5 для деталей. Конец.":           {"См. рис. 5 для деталей.", "Конец."},
		"Use e.g. this one. Then stop":             {"Use e.g. this one.", "Then stop"},
		"Конец... Начало снова. и строчная":        {"Конец...", "Начало снова. и строчная"},
		"Без точки в конце":                        {"Без точки в конце"},
		"":                                         nil,
		"Строка один.\nСтрока два.":                {"Строка один.", "Строка два."},
	}
	for in, want := range cases {
		if got := sents(in); !reflect.DeepEqual(got, want) {
			t.Errorf("%q:\n got %q\nwant %q", in, got, want)
		}
	}
}
