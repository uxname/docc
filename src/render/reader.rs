//! The reader: one UI for the documents and the work on them, built in the
//! Docspace frontend (npm run build:standalone) and embedded here as two files
//! by scripts/update-reader.sh, which writes the Docspace commit into VERSION.
//! docc watch serves it (it fetches /workspace.json and follows /events),
//! docc build --html inlines it with the workspace object.

const PAGE: &str = include_str!("reader/page.html");
const JS: &str = include_str!("reader/reader.js");
const CSS: &str = include_str!("reader/reader.css");
const VERSION: &str = include_str!("reader/VERSION");

/// reader_installed reports whether the embedded reader is the real bundle,
/// not the placeholder the repository starts with.
// ponytail: until the bundle is embedded, docc build --html keeps the older page (app.js)
// with the workspace inlined; drop it and this check when update-reader.sh has run.
pub fn reader_installed() -> bool {
    VERSION.trim() != "placeholder"
}

/// reader_page is the page of the reader around data: the script elements of
/// the data it inlines (the workspace object, the data island of the file);
/// without any the reader fetches /workspace.json.
pub fn reader_page(title: &str, lang: &str, data: &str) -> String {
    let data = data.to_string();
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    // one pass in the order of the shell: a value never meets a later placeholder
    let mut out = String::with_capacity(PAGE.len() + JS.len() + CSS.len() + data.len());
    let mut rest = PAGE;
    for (k, v) in [
        ("{{LANG}}", esc(lang)),
        ("{{VERSION}}", esc(VERSION.trim())),
        ("{{TITLE}}", esc(if title.is_empty() { "docc" } else { title })),
        ("{{CSS}}", CSS.to_string()),
        ("{{DATA}}", data),
        ("{{JS}}", JS.to_string()),
    ] {
        let Some(i) = rest.find(k) else { continue };
        out.push_str(&rest[..i]);
        out.push_str(&v);
        rest = &rest[i + k.len()..];
    }
    out.push_str(rest);
    out
}

/// workspace_script is the workspace object as the element the reader reads:
/// every "<" escaped, so nothing inside can end the element.
pub fn workspace_script(ws: &str) -> String {
    format!("<script type=\"application/json\" id=\"docc-workspace\">{}</script>\n", ws.replace('<', "\\u003c"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_inlines_the_workspace_once_and_safely() {
        let p = reader_page("A <b>", "ru", &workspace_script(r#"{"x":"</script><script>alert(1)</script>"}"#));
        assert!(p.contains("<title>A &lt;b&gt;</title>") && p.contains(r#"<html lang="ru">"#));
        assert_eq!(p.matches(r#"id="docc-workspace""#).count(), 1);
        assert!(!p.contains("</script><script>alert"));
        assert!(!reader_page("", "en", "").contains("docc-workspace\">"));
    }
}
