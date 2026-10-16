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
    ("Related", "Связанные"),
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
    (
        "No paragraph in the window to comment on.",
        "На экране нет абзаца для замечания.",
    ),
    ("Not sent yet", "Ещё не отправил"),
    (
        "There are changes since the version sent on",
        "Есть правки с отправленной версии от",
    ),
    ("Added:", "Добавлено:"),
    ("added text", "добавленный текст"),
    ("All changes viewed", "Все правки просмотрены"),
    ("Change", "Правка"),
    ("Changed:", "Изменено:"),
    ("Changed after you viewed it", "Изменено после просмотра"),
    ("changed paragraph", "изменённый абзац"),
    ("changes:", "правок:"),
    ("close", "закрыть"),
    ("Colours", "Цвета"),
    ("Comment", "Замечание"),
    ("commented fragment", "фрагмент с замечанием"),
    ("Comment on the selection", "Замечание к выделенному"),
    (
        "comment on the selection or the paragraph",
        "замечание к выделенному или к абзацу",
    ),
    ("Delete all my comments", "Удалить все мои замечания"),
    (
        "Download all as an archive (.zip)",
        "Скачать всё архивом (.zip)",
    ),
    (
        "Download all in one file (.md)",
        "Скачать всё одним файлом (.md)",
    ),
    ("Download this page (.md)", "Скачать страницу (.md)"),
    ("Edit navigation", "Переход по правкам"),
    ("Edits", "Правки"),
    ("Edits since", "Правки с"),
    ("Fragment changed", "Фрагмент изменён"),
    ("Got it", "Понятно"),
    ("Not now", "Скрыть"),
    ("History", "История"),
    ("Keys", "Клавиши"),
    ("Keys and colours", "Клавиши и цвета"),
    ("left", "осталось"),
    ("Left", "Осталось"),
    ("Marked as viewed.", "Отмечено как просмотренное."),
    (
        "mark viewed and go to the next",
        "отметить просмотренной и перейти к следующей",
    ),
    ("Mode", "Режим"),
    ("More actions", "Другие действия"),
    ("More comments:", "Ещё замечаний:"),
    ("new paragraph", "новый абзац"),
    ("next change", "следующая правка"),
    (
        "No comments yet. Select text or press «Comment» next to a paragraph.",
        "Замечаний пока нет. Выделите текст или нажмите «Замечание» у абзаца.",
    ),
    ("Not sent:", "Не отправлено:"),
    ("Overview of edits", "Обзор правок"),
    ("Page:", "Страница:"),
    ("previous change", "предыдущая правка"),
    ("Previous text", "Прежний текст"),
    ("Print", "Печать"),
    ("Reading", "Чтение"),
    ("Removed:", "Удалено:"),
    ("removed paragraph", "удалённый абзац"),
    ("Removed paragraph:", "Удалён абзац:"),
    ("removed text", "удалённый текст"),
    (
        "Saved in this browser only. Send your comments to the team from the Comments tab.",
        "Замечание сохранено только в этом браузере. Отправьте замечания команде на вкладке «Замечания».",
    ),
    ("save the comment", "сохранить замечание"),
    ("Search", "Поиск"),
    ("Send comments", "Отправить замечания"),
    ("sent", "отправлено"),
    ("Starting point", "Точка отсчёта"),
    ("Starting point…", "Точка отсчёта…"),
    ("Start review", "Начать просмотр"),
    (
        "The letter keys work in the Work mode, outside text fields.",
        "Буквенные клавиши работают в режиме «Работа», вне полей ввода.",
    ),
    ("There are changes since", "Есть правки с"),
    ("this help", "эта справка"),
    (
        "Turn on «Work» to leave comments.",
        "Включите «Работа», чтобы оставить замечания.",
    ),
    (
        "Turn on «Work» to see them and leave comments.",
        "Включите «Работа», чтобы увидеть их и оставить замечания.",
    ),
    ("viewed", "просмотрено"),
    ("Viewed", "Просмотрено"),
    ("Work", "Работа"),
    ("Your comment", "Ваше замечание"),
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
    ("No speech heard", "Речь не услышана"),
    (
        "Dictation failed, type the text instead",
        "Диктовка не сработала, напишите текст",
    ),
    ("Dictate", "Надиктовать"),
    ("Stop dictation", "Остановить диктовку"),
    ("From", "От кого"),
    ("Your name", "Ваше имя"),
    ("Cancel", "Отмена"),
    ("Save", "Сохранить"),
    ("Anonymous", "Без имени"),
    ("taken into account", "учтено"),
    ("the team is working on it", "команда разбирает"),
    ("not sent", "не отправлено"),
    ("Edit", "Изменить"),
    ("Delete", "Удалить"),
    ("Team's answer", "Ответ команды"),
    ("Comment deleted.", "Замечание удалено."),
    (
        "Discard the unsaved comment?",
        "Отбросить несохранённое замечание?",
    ),
    ("Undo", "Вернуть"),
    ("History of the paragraph", "История абзаца"),
    ("Close", "Закрыть"),
    ("first version", "первая версия"),
    ("History of the page", "История страницы"),
    (
        "No changes in this file's history.",
        "В истории этого файла изменений нет.",
    ),
    ("page created", "страница создана"),
    ("Changed", "Изменено"),
    ("Added", "Добавлено"),
    ("Removed", "Удалено"),
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
    ("Comments", "Замечания"),
    ("Comments on", "Замечания к документу"),
    ("For the team, do not edit:", "Для команды, не изменяйте:"),
    (
        "Copied. Paste it into a message to the team.",
        "Скопировано. Вставьте в сообщение команде.",
    ),
    (
        "Could not copy: download the file instead.",
        "Не удалось скопировать: скачайте файл.",
    ),
    (
        "Comments are kept only in this browser until you send them to the team.",
        "Замечания хранятся только в этом браузере, пока вы не отправите их команде.",
    ),
    ("Copy for sending", "Скопировать для отправки"),
    ("Download as a file", "Скачать файлом"),
    (
        "Delete all comments of this browser?",
        "Удалить все замечания этого браузера?",
    ),
    ("the version sent on", "отправленной версии от"),
    ("Show changes since", "Показать изменения с"),
    ("do not show", "не показывать"),
    ("No changes", "Изменений нет"),
    ("Previous change", "Предыдущее изменение"),
    ("Next change", "Следующее изменение"),
    ("of", "из"),
    ("Navigation", "Навигация"),
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
