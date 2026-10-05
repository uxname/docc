package render

import (
	"html"
	"regexp"
	"strings"
)

// ru translates the fixed labels of the output ("lang": "ru" in docc.json).
// The English text is the key, so the default build needs no table and stays
// as it was. page.html and page.js mark their labels as {{t:English text}}.
// Every key must have a Russian entry: TestLabelsTranslated checks it.
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
	"earlier":              "выше",
	"more":                 "ещё",
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

var labelRe = regexp.MustCompile(`\{\{t:([^}]+)\}\}`)

// Labels returns the {{t:...}} keys of a template.
func Labels(s string) []string {
	var keys []string
	for _, m := range labelRe.FindAllStringSubmatch(s, -1) {
		keys = append(keys, m[1])
	}
	return keys
}

var jsQuote = strings.NewReplacer(`\`, `\\`, `'`, `\'`)

// labels replaces {{t:...}} in page.html (js false) or page.js (js true) with
// the label in the language of the project, escaped for that file.
func (r *Renderer) labels(s string, js bool) string {
	return labelRe.ReplaceAllStringFunc(s, func(m string) string {
		v := r.t(labelRe.FindStringSubmatch(m)[1])
		if js {
			return jsQuote.Replace(v)
		}
		return html.EscapeString(v)
	})
}
