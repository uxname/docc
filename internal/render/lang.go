package render

import (
	"html"
	"regexp"
	"strings"
)

// ru translates the fixed labels of the output ("lang": "ru" in docc.json).
// The English text is the key, so the default build needs no table and stays
// as it was. page.html and app.js mark their labels as {{t:English text}}.
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
	// app.js: history, changes, comments, export
	"en": "ru",
	"This documentation is an application: turn on JavaScript to read it, or read the Markdown in the output folder.": "Эта документация — приложение: включите JavaScript, чтобы её читать, или читайте Markdown в папке сборки.",
	"not saved yet":               "ещё не сохранено",
	"removed:":                    "удалено:",
	"added:":                      "добавлено:",
	"No access to the microphone": "Нет доступа к микрофону",
	"Speech service unreachable":  "Сервис распознавания речи недоступен",
	"No speech heard":             "Речь не услышана",
	"Dictation failed, type the text instead": "Диктовка не сработала, напишите текст",
	"Dictate":                            "Надиктовать",
	"Stop dictation":                     "Остановить диктовку",
	"From":                               "От кого",
	"Your name":                          "Ваше имя",
	"Comment on this paragraph":          "Замечание к этому абзацу",
	"Cancel":                             "Отмена",
	"Save":                               "Сохранить",
	"Anonymous":                          "Без имени",
	"taken into account":                 "учтено",
	"the team is working on it":          "команда разбирает",
	"not sent":                           "не отправлено",
	"Edit":                               "Изменить",
	"Delete":                             "Удалить",
	"Team's answer":                      "Ответ команды",
	"Comment deleted.":                   "Замечание удалено.",
	"Undo":                               "Вернуть",
	"History of the paragraph":           "История абзаца",
	"Close":                              "Закрыть",
	"first version":                      "первая версия",
	"Was → now":                          "Было → стало",
	"new":                                "новое",
	"What changed":                       "Что изменилось",
	"History of the page":                "История страницы",
	"No changes in this file's history.": "В истории этого файла изменений нет.",
	"page created":                       "страница создана",
	"Changed":                            "Изменено",
	"Added":                              "Добавлено",
	"Removed":                            "Удалено",
	"Page history":                       "История страницы",
	"Nothing is found at this link:":     "По этой ссылке ничего не найдено:",
	"The first page is shown; find the place in the tree or the search.": "Открыта первая страница; найдите нужное место в дереве или поиском.",
	"Removed from this page since the chosen version":                    "Удалено с этой страницы после выбранной версии",
	"Changes on the page":                  "Изменений на странице",
	"Found":                                "Найдено",
	"the first ones are shown, add a word": "показаны первые, уточните запрос",
	"Comments":                             "Замечания",
	"Your comments":                        "Ваши замечания",
	"Comments on":                          "Замечания к документу",
	"For the team, do not edit:":           "Для команды, не изменяйте:",
	"Copied. Paste it into a message to the team.":                            "Скопировано. Вставьте в сообщение команде.",
	"Could not copy: download the file instead.":                              "Не удалось скопировать: скачайте файл.",
	"Comments are kept only in this browser until you send them to the team.": "Замечания хранятся только в этом браузере, пока вы не отправите их команде.",
	"All comments are sent.":                                                  "Все замечания отправлены.",
	"Copy for sending":                                                        "Скопировать для отправки",
	"Download as a file":                                                      "Скачать файлом",
	"Delete all comments of this browser?":                                    "Удалить все замечания этого браузера?",
	"Delete all":                                                              "Удалить все",
	"No comments yet. Point at a paragraph and press 💬.":                      "Замечаний пока нет. Наведите на абзац и нажмите 💬.",
	"No comments yet. Tap 💬 next to a paragraph to write one.":                "Замечаний пока нет. Нажмите 💬 под абзацем, чтобы написать.",
	"Received by the team":                                                    "Получено командой",
	"in work":                                                                 "в работе",
	"Changes":                                                                 "Изменения",
	"Changes by page":                                                         "Изменения по страницам",
	"changed":                                                                 "изменено",
	"removed":                                                                 "удалено",
	"This page (.md)":                                                         "Эта страница (.md)",
	"All pages in one file (.md)":                                             "Все страницы одним файлом (.md)",
	"All pages as separate files (.zip)":                                      "Все страницы отдельными файлами (.zip)",
	"the version sent on":                                                     "отправленной версии от",
	"Show changes since":                                                      "Показать изменения с",
	"do not show":                                                             "не показывать",
	"Changed paragraphs":                                                      "Изменено абзацев",
	"pages":                                                                   "страниц",
	"Only removals":                                                           "Только удаления",
	"No changes":                                                              "Изменений нет",
	"Previous change":                                                         "Предыдущее изменение",
	"Next change":                                                             "Следующее изменение",
	"of":                                                                      "из",
	"Version of":                                                              "Версия от",
	"Export":                                                                  "Экспорт",
	"Navigation":                                                              "Навигация",
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

// labels replaces {{t:...}} in page.html (js false) or app.js (js true) with
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
