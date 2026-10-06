package main

import (
	"strings"
	"testing"
)

const brandSVG = `<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="2" fill="#fff"/></svg>`

func TestBrandingInHTMLOnly(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{"branding":{"name":"A & B","url":"https://example.com/x","logo":"docs/logo.svg",
			"accent":"#57e9c7","accentLight":"#0b7a64","header":"#1f1f1f"}}`,
		"docs/logo.svg":       brandSVG,
		"docs/src/index.json": `{"title":"Index","body":"Text."}`,
	})
	if code, out, errs := do("build", "--html"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	html := readFile(t, "docs/out.html")
	for _, want := range []string{
		`<a class="brand" href="https://example.com/x"><span class="logo" role="img" aria-label="A &amp; B"></span></a>`,
		`--brand-logo:url("data:image/svg+xml;base64,`, `--brand-bg:#1f1f1f`, `--accent:#0b7a64`, `--accent:#57e9c7`,
		`<div class="brand-print">A &amp; B</div>`,
		`<footer class="brand-foot">A &amp; B · <a href="https://example.com/x">example.com</a></footer>`,
	} {
		if !strings.Contains(html, want) {
			t.Errorf("out.html lacks %q", want)
		}
	}
	if strings.Count(html, `--brand-logo:url(`) != 1 {
		t.Error("the logo must be embedded once")
	}
	if md := readFile(t, "docs/out/index.md"); strings.Contains(md, "A & B") {
		t.Errorf("Markdown is branded:\n%s", md)
	}
}

func TestBrandingNameOnly(t *testing.T) {
	project(t, map[string]string{
		"docc.json":           `{"branding":{"name":"Acme"}}`,
		"docs/src/index.json": `{"title":"Index","body":"Text."}`,
	})
	if code, out, errs := do("build", "--html"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	html := readFile(t, "docs/out.html")
	for _, want := range []string{`<span class="brand">Acme</span>`, `<footer class="brand-foot">Acme</footer>`} {
		if !strings.Contains(html, want) {
			t.Errorf("out.html lacks %q", want)
		}
	}
	if !strings.Contains(html, ":root{--brand-bg:#1f1f1f;--brand-line:var(--accent)}\n#side .brand") || strings.Contains(html, "--brand-logo:") {
		t.Error("no logo and no colours were given: only the default header colour is set")
	}
}

func TestBrandingErrors(t *testing.T) {
	for cfg, want := range map[string]string{
		`{"branding":{}}`: "needs a name",
		`{"branding":{"name":"A","logo":"nope.svg"}}`:        "does not exist",
		`{"branding":{"name":"A","logo":"docs/logo.txt"}}`:   "png, jpg",
		`{"branding":{"name":"A","accent":"red"}}`:           "accent must be a colour",
		`{"branding":{"name":"A","header":"#12345"}}`:        "header must be a colour",
		`{"branding":{"name":"A","url":"javascript:alert"}}`: "http or https",
		`{"branding":{"name":"A","color":"#fff"}}`:           "unknown key",
	} {
		project(t, map[string]string{"docc.json": cfg, "docs/logo.txt": "x", "docs/src/index.json": `{"title":"I"}`})
		code, out, errs := do("validate")
		if code != 2 || !strings.Contains(out+errs, want) {
			t.Errorf("%s: %d %q %q, want %q", cfg, code, out, errs, want)
		}
	}
}
