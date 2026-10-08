package render

import (
	_ "embed"
	"encoding/base64"
	"net/url"
	"os"
	"path"
	"strings"

	"github.com/uxname/docc/internal/engine"
)

//go:embed brand.css
var brandCSS string

// brand returns the pieces the branding section adds to the HTML page: CSS,
// the header (placed in the sidebar and in the mobile bar), the line at the
// top of the printed first page and the footer. All four are empty without
// the section, so an unbranded build stays as it was.
func (r *Renderer) brand() (css, head, print, foot string) {
	b := r.P.Cfg.Brand
	if b == nil {
		return
	}
	bg := orDefault(b.Header, "#1f1f1f")
	light, dark := orDefault(b.AccentLight, b.Accent), orDefault(b.Accent, b.AccentLight)
	vars := "--brand-bg:" + bg + ";--brand-line:" + orDefault(b.Accent, "var(--accent)")
	logo := ""
	if b.Logo != "" {
		if data, err := os.ReadFile(r.P.Cfg.Abs(b.Logo)); err == nil { // the config checked that it exists
			vars += `;--brand-logo:url("data:` + engine.ImageExt[strings.ToLower(path.Ext(b.Logo))] + ";base64," + base64.StdEncoding.EncodeToString(data) + `")`
			logo = `<span class="logo" role="img" aria-label="` + esc(b.Name) + `"></span>`
		}
	}
	css = ":root{" + vars
	if light != "" {
		css += ";--accent:" + light + ";--accent-bg:color-mix(in srgb," + light + " 12%,#fff)"
	}
	css += "}\n"
	if dark != "" {
		css += "@media (prefers-color-scheme:dark){:root{--accent:" + dark + ";--accent-bg:color-mix(in srgb," + dark + " 20%,#0f1318)}}\n"
	}
	css += brandCSS

	name := esc(b.Name)
	label := orDefault(logo, name)
	if b.URL != "" {
		head = `<a class="brand" href="` + esc(b.URL) + `">` + label + `</a>`
	} else {
		head = `<span class="brand">` + label + `</span>`
	}
	print = `<div class="brand-print">` + name + `</div>`
	foot = `<footer class="brand-foot">` + name
	if u, err := url.Parse(b.URL); err == nil && u.Host != "" {
		foot += ` · <a href="` + esc(b.URL) + `">` + esc(u.Host) + `</a>`
	}
	foot += `</footer>`
	return
}

func orDefault(s, def string) string {
	if s == "" {
		return def
	}
	return s
}
