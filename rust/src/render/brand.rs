use base64::Engine as _;

use crate::engine;

use super::lang::html_escape as esc;
use super::render::Renderer;
use super::text::path_ext;

static BRAND_CSS: &str = include_str!("../../../internal/render/brand.css");

impl Renderer<'_> {
    /// brand returns the pieces the branding section adds to the HTML page: CSS,
    /// the header (placed in the sidebar and in the mobile bar), the line at the
    /// top of the printed first page and the footer. All four are empty without
    /// the section, so an unbranded build stays as it was.
    pub(super) fn brand(&self) -> (String, String, String, String) {
        let Some(b) = &self.p.cfg.brand else {
            return Default::default();
        };
        let bg = or_default(&b.header, "#1f1f1f");
        let (light, dark) = (
            or_default(&b.accent_light, &b.accent),
            or_default(&b.accent, &b.accent_light),
        );
        let mut vars = format!(
            "--brand-bg:{bg};--brand-line:{}",
            or_default(&b.accent, "var(--accent)")
        );
        let mut logo = String::new();
        if !b.logo.is_empty() {
            // the config checked that it exists
            if let Ok(data) = std::fs::read(self.p.cfg.abs(&b.logo)) {
                vars += &format!(
                    r#";--brand-logo:url("data:{};base64,{}")"#,
                    engine::image_ext(&path_ext(&b.logo).to_ascii_lowercase()).unwrap_or(""),
                    base64::engine::general_purpose::STANDARD.encode(data)
                );
                logo = format!(
                    r#"<span class="logo" role="img" aria-label="{}"></span>"#,
                    esc(&b.name)
                );
            }
        }
        let mut css = format!(":root{{{vars}");
        if !light.is_empty() {
            css += &format!(";--accent:{light};--accent-bg:color-mix(in srgb,{light} 12%,#fff)");
        }
        css += "}\n";
        if !dark.is_empty() {
            css += &format!(
                "@media (prefers-color-scheme:dark){{:root{{--accent:{dark};--accent-bg:color-mix(in srgb,{dark} 20%,#0f1318)}}}}\n"
            );
        }
        css += BRAND_CSS;

        let name = esc(&b.name);
        let label = or_default(&logo, &name);
        let head = if !b.url.is_empty() {
            format!(r#"<a class="brand" href="{}">{label}</a>"#, esc(&b.url))
        } else {
            format!(r#"<span class="brand">{label}</span>"#)
        };
        let print = format!(r#"<div class="brand-print">{name}</div>"#);
        let mut foot = format!(r#"<footer class="brand-foot">{name}"#);
        let host = url_host(&b.url);
        if !host.is_empty() {
            foot += &format!(r#" · <a href="{}">{}</a>"#, esc(&b.url), esc(host));
        }
        foot += "</footer>";
        (css, head, print, foot)
    }
}

fn or_default<'s>(s: &'s str, def: &'s str) -> &'s str {
    if s.is_empty() { def } else { s }
}

/// url_host is url.Parse(raw).Host for a URL the config accepted (http or
/// https with a host): the authority after any user info, as written.
// ponytail: no %-unescaping of the host; the config accepts no such host in practice.
fn url_host(raw: &str) -> &str {
    let u = raw.split_once('#').map_or(raw, |(u, _)| u);
    let u = u.split_once('?').map_or(u, |(u, _)| u);
    let Some((_, rest)) = u.split_once(':') else {
        return "";
    };
    let Some(auth) = rest.strip_prefix("//") else {
        return "";
    };
    let auth = auth.find('/').map_or(auth, |i| &auth[..i]);
    auth.rfind('@').map_or(auth, |i| &auth[i + 1..])
}
