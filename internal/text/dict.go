package text

import "strings"

// Built-in dictionaries (Russian and English); docc.json extends them.
var (
	// Forbidden: DOC301.
	Forbidden = []string{
		"быстро", "удобно", "интуитивно", "и т.д.", "и т.п.",
		"fast", "easy", "intuitive", "user-friendly", "etc.",
	}
	// Hedging: DOC352.
	Hedging = []string{
		"возможно", "вероятно", "как правило", "по-видимому", "в некоторых случаях", "наверное", "скорее всего",
		"несколько", "нескольких", "нескольким", "несколькими",
		"некоторые", "некоторых", "некоторым", "некоторыми", "различные", "различных", "различным", "различными",
		"maybe", "probably", "usually", "might", "perhaps", "several", "various",
	}
	// Timeless: DOC353.
	Timeless = []string{
		"сейчас", "недавно", "пока", "в настоящее время", "скоро", "в будущем", "новый", "новая", "новое", "новые",
		"currently", "recently", "soon", "new", "at the moment",
	}
	// Placeholders: DOC303.
	// A value that is only a dash ("—", "-", "–") is one too; see IsDashOnly.
	Placeholders = []string{"TODO", "TBD", "FIXME", "lorem", "???", "N/A", "см. выше", "работает корректно", "по необходимости", "see above", "works correctly"}
)

// IsDashOnly reports whether a value is only a dash: a placeholder for
// "nothing to say" (DOC303).
func IsDashOnly(s string) bool {
	switch strings.TrimSpace(s) {
	case "—", "-", "–":
		return true
	}
	return false
}
