package text

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
	// Pronouns: DOC354, first word of a value without an explanation.
	Pronouns = []string{
		"Это", "Он", "Она", "Оно", "Они", "Данный", "Данная", "Данное", "Данные", "Такой", "Такая",
		"This", "It", "They", "These", "That",
	}
	// Placeholders: DOC303.
	Placeholders = []string{"TODO", "TBD", "FIXME", "lorem", "???"}
)
