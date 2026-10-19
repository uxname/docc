use std::collections::HashMap;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::render::Renderer;

/// ru translates the fixed labels of the output ("lang": "ru" in docc.json).
/// The English text is the key, so the default build needs no table and stays
/// as it was. page.html and app.js mark their labels as {{t:English text}}.
/// Every key must have a Russian entry: labels_translated checks it.
pub(super) const RU: &[(&str, &str)] = &[
    // Markdown pages and the document tree
    ("Parent", "Родитель"),
    ("Referenced by", "Ссылаются"),
    ("Covered by", "Покрыто кодом"),
    ("Document tree", "Дерево документов"),
    ("Sections of the page", "Разделы страницы"),
    ("Without parent", "Без родителя"),
    (
        "Documents without code coverage",
        "Документы без покрытия кодом",
    ),
    ("Hidden document.", "Скрытый документ."),
    ("Service information.", "Служебная информация."),
    ("Notes (hidden)", "Заметки (скрыто)"),
    ("Suppressions (docIgnore)", "Подавления (docIgnore)"),
    ("hidden", "скрыт"),
    ("priority", "приоритет"),
    ("include", "вставка"),
    (
        "full build: includes hidden documents and notes — do not publish",
        "полная сборка: со скрытыми документами и заметками — не публиковать",
    ),
    // HTML
    ("Pages in this section", "Страницы раздела"),
    ("Previous", "Назад"),
    ("Next", "Далее"),
    ("Neighbouring pages", "Соседние страницы"),
    ("Breadcrumb", "Путь"),
    ("Pages", "Страницы"),
    ("Collapse", "Свернуть"),
    ("Expand", "Развернуть"),
    (
        "Full build: includes hidden documents and notes. Do not publish.",
        "Полная сборка: со скрытыми документами и заметками. Не публиковать.",
    ),
    // page.html
    ("Skip to content", "К содержанию"),
    ("Menu", "Меню"),
    ("Search  ( / )", "Поиск  ( / )"),
    ("Search the documentation", "Поиск по документации"),
    ("Search results", "Результаты поиска"),
    // page.js
    ("Link to this section", "Ссылка на этот раздел"),
    ("Copy", "Копировать"),
    ("Copied", "Скопировано"),
    ("Failed", "Не удалось"),
    ("Nothing found", "Ничего не найдено"),
    ("earlier", "выше"),
    ("more", "ещё"),
    // app.js: history, changes, comments, export
    ("Table", "Таблица"),
    ("Added:", "Добавлено:"),
    ("Changed:", "Изменено:"),
    ("changed paragraph", "изменённый абзац"),
    ("Comment", "Комментарий"),
    ("Comment on the selection", "Комментировать выделенное"),
    ("Fragment changed", "Фрагмент изменён"),
    ("More actions", "Другие действия"),
    ("More comments:", "Ещё комментариев:"),
    ("new paragraph", "новый абзац"),
    ("Previous text", "Прежний текст"),
    ("Removed:", "Удалено:"),
    ("removed paragraph", "удалённый абзац"),
    ("Removed paragraph:", "Удалён абзац:"),
    ("Search", "Поиск"),
    ("Your comment", "Ваш комментарий"),
    ("en", "ru"),
    (
        "This documentation is an application: turn on JavaScript to read it, or read the Markdown in the output folder.",
        "Эта документация — приложение: включите JavaScript, чтобы её читать, или читайте Markdown в папке сборки.",
    ),
    ("not saved yet", "ещё не сохранено"),
    ("removed:", "удалено:"),
    ("added:", "добавлено:"),
    ("No access to the microphone", "Нет доступа к микрофону"),
    (
        "Speech service unreachable",
        "Сервис распознавания речи недоступен",
    ),
    (
        "Dictation failed, type the text instead",
        "Диктовка не сработала, напишите текст",
    ),
    ("Dictate", "Надиктовать"),
    ("From", "От кого"),
    ("Your name", "Ваше имя"),
    ("Cancel", "Отмена"),
    ("Save", "Сохранить"),
    ("Anonymous", "Без имени"),
    ("Edit", "Изменить"),
    ("Delete", "Удалить"),
    ("Comment deleted.", "Комментарий удалён."),
    (
        "Discard the unsaved comment?",
        "Отбросить несохранённый комментарий?",
    ),
    ("Undo", "Вернуть"),
    ("Close", "Закрыть"),
    (
        "Nothing is found at this link:",
        "По этой ссылке ничего не найдено:",
    ),
    (
        "The first page is shown; find the place in the tree or the search.",
        "Открыта первая страница; найдите нужное место в дереве или поиском.",
    ),
    ("Found", "Найдено"),
    (
        "the first ones are shown, add a word",
        "показаны первые, уточните запрос",
    ),
    ("Comments", "Комментарии"),
    ("Comments on", "Комментарии к документу"),
    (
        "Could not copy: download the file instead.",
        "Не удалось скопировать: скачайте файл.",
    ),
    ("Download as a file", "Скачать файлом"),
    ("Navigation", "Навигация"),
    // timeline, comments, reset
    ("Answer", "Ответ"),
    ("Back to now", "Вернуться к сейчас"),
    (
        "Comments can be left on the current text only.",
        "Комментарии оставляются только к текущему тексту.",
    ),
    (
        "Copied. Paste it into a message.",
        "Скопировано. Вставьте в сообщение.",
    ),
    ("Copy comments", "Скопировать комментарии"),
    (
        "Delete from this browser your name, the chosen point of the timeline, the open tab and your comments",
        "Удалить из этого браузера ваше имя, выбранное положение шкалы, открытую вкладку и ваши комментарии",
    ),
    ("edited", "изменён"),
    ("Edits since:", "Правки с:"),
    ("For the import, do not edit:", "Для импорта, не изменяйте:"),
    ("new", "новый"),
    (
        "No comments yet. Select text on the page to comment on it.",
        "Комментариев пока нет. Выделите текст на странице, чтобы прокомментировать его.",
    ),
    (
        "No edits in the chosen period. Open the timeline above the page and pick an earlier save or date to see them.",
        "За выбранный период правок нет. Откройте шкалу над страницей и выберите более раннее сохранение или дату, чтобы их увидеть.",
    ),
    ("now, edits are not shown", "сейчас — правки не подсвечены"),
    ("now", "сейчас"),
    ("now, not saved yet", "сейчас, ещё не сохранено"),
    (
        "Recording, press to stop",
        "Идёт запись — нажмите, чтобы остановить",
    ),
    ("removed", "удалён"),
    ("Reset everything…", "Сбросить всё…"),
    (
        "Saved in this browser only. Copy your comments in the Comments tab to pass them on.",
        "Комментарий сохранён только в этом браузере. Чтобы передать комментарии, скопируйте их на вкладке «Комментарии».",
    ),
    ("Text as of:", "Текст на:"),
    (
        "The documentation has no saved history yet.",
        "У документации ещё нет сохранённой истории.",
    ),
    ("Timeline", "Шкала времени"),
    ("Show more:", "Показать ещё"),
    ("of", "из"),
    (
        "It is put into your comments, so whoever keeps the documentation knows who wrote them. You can change it in ⋯.",
        "Оно подставится в ваши комментарии, чтобы тот, кто ведёт документацию, знал, кто их написал. Изменить его можно в «⋯».",
    ),
    ("not given", "не указано"),
    ("change", "изменить"),
    ("What is your name?", "Как вас зовут?"),
    ("Your name:", "Ваше имя:"),
    ("Skip", "Пропустить"),
    ("Edits:", "Правки:"),
    (
        "Pick a save to see what it changed. Marked saves changed this page; the shaded ones are drawn in the text.",
        "Нажмите на сохранение, чтобы увидеть, что в нём изменилось. «Эта страница» — сохранения, менявшие эту страницу; подсвеченные показаны в тексте.",
    ),
    ("Edits after this save", "Правки после этого сохранения"),
    ("Text as of the date", "Текст на дату"),
    ("Only saves that changed this page", "Только сохранения, менявшие эту страницу"),
    ("Saves:", "Сохранений:"),
    ("edits are not shown", "правки не подсвечены"),
    ("this page", "эта страница"),
    (
        "You are looking at the page as it was",
        "Вы смотрите страницу такой, какой она была",
    ),
    (
        "Your comments are kept only in this browser. Copy them or download a file and pass it to whoever keeps the documentation.",
        "Ваши комментарии хранятся только в этом браузере. Скопируйте их или скачайте файлом и передайте тому, кто ведёт документацию.",
    ),
    ("New page", "Новая страница"),
    ("Edits on the page", "Правки на странице"),
    ("Edits on the page:", "Правки на странице:"),
    ("No edits on this page", "На этой странице правок нет"),
    ("Next page with edits:", "Следующая страница с правками:"),
    ("Edits inside", "Внутри есть правки"),
    ("page removed", "страница удалена"),
    ("paragraph removed", "абзац удалён"),
    ("It was left on:", "Оставлен к тексту:"),
    ("Open the page", "Открыть страницу"),
];

static RU_MAP: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| RU.iter().copied().collect());

impl Renderer<'_> {
    /// t returns a fixed label in the language of the project.
    pub(super) fn t(&self, s: &str) -> String {
        if self.p.cfg.lang == "ru"
            && let Some(v) = RU_MAP.get(s)
        {
            return v.to_string();
        }
        s.to_string()
    }

    /// labels replaces {{t:...}} in page.html (js false) or app.js (js true) with
    /// the label in the language of the project, escaped for that file.
    pub(super) fn labels(&self, s: &str, js: bool) -> String {
        LABEL_RE
            .replace_all(s, |m: &Captures| {
                let v = self.t(&m[1]);
                if js {
                    return v.replace('\\', r"\\").replace('\'', r"\'");
                }
                html_escape(&v)
            })
            .into_owned()
    }
}

static LABEL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{t:([^}]+)\}\}").unwrap());

/// Labels returns the {{t:...}} keys of a template.
pub fn labels(s: &str) -> Vec<String> {
    LABEL_RE
        .captures_iter(s)
        .map(|m| m[1].to_string())
        .collect()
}

/// html_escape is html.EscapeString: <, >, &, ' and ".
pub(super) fn html_escape(s: &str) -> String {
    let mut b = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => b.push_str("&lt;"),
            '>' => b.push_str("&gt;"),
            '&' => b.push_str("&amp;"),
            '\'' => b.push_str("&#39;"),
            '"' => b.push_str("&#34;"),
            _ => b.push(c),
        }
    }
    b
}
