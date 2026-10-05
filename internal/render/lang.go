package render

import "strings"

// ru translates the fixed labels of the output ("lang": "ru" in docc.json).
// The English text is the key, so the default build needs no table and stays
// as it was. page.html and page.js keep their English text; localize replaces
// the quoted labels there.
var ru = map[string]string{
	// Markdown pages and the document tree
	"Parent":                          "Родитель",
	"Referenced by":                   "Ссылаются",
	"Covered by":                      "Покрыто кодом",
	"Document tree":                   "Дерево документов",
	"Without parent":                  "Без родителя",
	"Documents without code coverage": "Документы без покрытия кодом",
	"Hidden document.":                "Скрытый документ.",
	"Service information.":            "Служебная информация.",
	"Notes (hidden)":                  "Заметки (скрыто)",
	"Suppressions (docIgnore)":        "Подавления (docIgnore)",
	"hidden":                          "скрыт",
	"priority":                        "приоритет",
	"include":                         "вставка",
	"full build: includes hidden documents and notes — do not publish": "полная сборка: со скрытыми документами и заметками — не публиковать",
	// HTML
	"Pages in this section": "Страницы раздела",
	"Related":               "Связанные",
	"Previous":              "Назад",
	"Next":                  "Далее",
	"Neighbouring pages":    "Соседние страницы",
	"Breadcrumb":            "Путь",
	"Pages":                 "Страницы",
	"Collapse":              "Свернуть",
	"Expand":                "Развернуть",
	"Full build: includes hidden documents and notes. Do not publish.": "Полная сборка: со скрытыми документами и заметками. Не публиковать.",
	// page.html
	"Skip to content":          "К содержанию",
	"Menu":                     "Меню",
	"Search  ( / )":            "Поиск  ( / )",
	"Search the documentation": "Поиск по документации",
	"Search results":           "Результаты поиска",
	"On this page":             "На этой странице",
	// page.js
	"Link to this section": "Ссылка на этот раздел",
	"Copy":                 "Копировать",
	"Copied":               "Скопировано",
	"Failed":               "Не удалось",
	"Nothing found":        "Ничего не найдено",
	" earlier":             " выше",
	" more ":               " ещё ",
}

// t returns a fixed label in the language of the project.
func (r *Renderer) t(s string) string {
	if r.P.Cfg.Lang == "ru" {
		if v, ok := ru[s]; ok {
			return v
		}
	}
	return s
}

// localize replaces the labels of page.html ("x" and >x<) or page.js ('x')
// written between the given quote pairs.
func (r *Renderer) localize(s string, quotes ...string) string {
	if r.P.Cfg.Lang != "ru" {
		return s
	}
	var pairs []string
	for en, tr := range ru {
		for i := 0; i+1 < len(quotes); i += 2 {
			pairs = append(pairs, quotes[i]+en+quotes[i+1], quotes[i]+tr+quotes[i+1])
		}
	}
	return strings.NewReplacer(pairs...).Replace(s)
}
