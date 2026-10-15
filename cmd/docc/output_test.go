package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"testing"
)

func TestLangRu(t *testing.T) {
	project(t, map[string]string{
		"docc.json":           `{"lang":"ru"}`,
		"docs/src/index.json": `{"title":"Главная","body":"См. [[child]]."}`,
		"docs/src/child.json": `{"title":"Раздел","parent":"index","priority":1}`,
		"docs/src/loose.json": `{"title":"Ничей","hidden":true}`,
	})
	if code, out, errs := do("build", "--html"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	child, index, tree := readFile(t, "docs/out/child.md"), readFile(t, "docs/out/index.md"), readFile(t, "docs/out/_tree.md")
	for _, want := range []string{"page · приоритет 1", "## Родитель\n\n[Главная](index.md)", "## Ссылаются"} {
		if !strings.Contains(child, want) {
			t.Errorf("child.md lacks %q:\n%s", want, child)
		}
	}
	if !strings.Contains(index, "Дерево документов: [_tree.md](_tree.md)") || !strings.Contains(tree, "# Дерево документов") {
		t.Errorf("tree label:\n%s\n%s", index, tree)
	}
	html := readFile(t, "docs/out.html")
	for _, want := range []string{
		">Страницы раздела<", ">Связанные<", "<small>Далее</small>", "<small>Назад</small>", `<html lang="ru">`,
		`placeholder="Поиск  ( / )"`, `aria-label="Переход по правкам"`, `'Копировать'`, `'Ничего не найдено'`, " ещё ↓", "🎤 Надиктовать",
		"Скопировать для отправки", "Показать изменения с",
	} {
		if !strings.Contains(html, want) {
			t.Errorf("out.html lacks %q", want)
		}
	}
	for _, bad := range []string{"Pages in this section", "'Copy'", "'On this page'", `aria-label="On this page"`, ">Parent<", "{{t:"} {
		if strings.Contains(html, bad) {
			t.Errorf("out.html keeps %q", bad)
		}
	}
	if code, out, _ := do("build", "--full"); code != 0 || !strings.Contains(readFile(t, "docs/out-full/loose.md"), "> Скрытый документ.") ||
		!strings.Contains(readFile(t, "docs/out-full/_tree.md"), "## Без родителя") {
		t.Fatalf("full build labels: %d %s", code, out)
	}
	if err := os.WriteFile("docc.json", []byte(`{"lang":"de"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if code, _, errs := do("check"); code != 2 || !strings.Contains(errs, `lang must be "en" or "ru"`) {
		t.Fatalf("%d %s", code, errs)
	}
}

func TestImages(t *testing.T) {
	png := "\x89PNG\r\n\x1a\n\x00 bytes"
	project(t, map[string]string{
		"docs/src/img/a.png":      png,
		"docs/src/img/unused.png": "x",
		"docs/src/index.json":     `{"title":"Index","body":["Own ![A](img/a.png) and code ` + "`![C](nope.png)`" + `.","{{p#body}}"]}`,
		"docs/src/sub/p.json":     `{"title":"P","parent":"index","body":"From sub ![B](../img/a.png \"t\")."}`,
	})
	if code, out, errs := do("build", "--single", "--html"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	if got := readFile(t, "docs/out/img/a.png"); got != png {
		t.Fatalf("copy differs: %q", got)
	}
	if _, err := os.Stat("docs/out/img/unused.png"); err == nil {
		t.Error("an image no page shows is copied")
	}
	checks := map[string][]string{
		"docs/out/index.md": {"Own ![A](img/a.png)", "`![C](nope.png)`", "From sub ![B](img/a.png \"t\")"}, // the include is rebased
		"docs/out/sub/p.md": {"![B](../img/a.png \"t\")"},
		"docs/out.md":       {"![A](out/img/a.png)", "![B](out/img/a.png \"t\")"},
		"docs/out.html":     {`src="data:image/png;base64,` + base64.StdEncoding.EncodeToString([]byte(png)) + `"`},
	}
	for f, wants := range checks {
		got := readFile(t, f)
		if f == "docs/out.html" {
			got = readApp(t, f).Blocks
		}
		for _, want := range wants {
			if !strings.Contains(got, want) {
				t.Errorf("%s lacks %q", f, want)
			}
		}
	}
	if code, out, _ := do("check"); code != 0 {
		t.Fatalf("fresh build must pass check: %s", out)
	}
	// a copy that differs only by line endings is stale: images compare byte for byte
	if err := os.WriteFile("docs/out/img/a.png", []byte(strings.ReplaceAll(png, "\r\n", "\n")), 0o644); err != nil {
		t.Fatal(err)
	}
	if code, out, _ := do("check"); code != 1 || !strings.Contains(out, "docs/out/img/a.png") || !strings.Contains(out, "DOC501") {
		t.Fatalf("%d %s", code, out)
	}
}

func TestFootnotesInHTML(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index","body":["Claim[^1].","[^1]: Source one."]}`,
		"docs/src/p.json":     `{"title":"P","parent":"index","body":["Other[^1].","[^1]: Source two."]}`,
	})
	if code, out, errs := do("build", "--html"); code != 0 {
		t.Fatalf("%d %s %s", code, out, errs)
	}
	html := readApp(t, "docs/out.html").Blocks
	for _, want := range []string{`id="index--fn:1"`, `href="#index--fn:1"`, `id="p--fn:1"`, `href="#p--fnref:1"`, "Source two."} {
		if !strings.Contains(html, want) {
			t.Errorf("out.html lacks %q", want)
		}
	}
}

// Custom rules and DOC313 see "й" typed as и + U+0306 like the composed
// letter, and report the column of the source text.
func TestCustomRulesAndRepeatsInNFC(t *testing.T) {
	project(t, map[string]string{
		"docc.json": `{"customRules":[
			{"id":"custom/no-new","level":"warn","given":"/body/*","forbid":"новый","message":"no new"},
			{"id":"custom/needs-key","level":"warn","given":"/key","require":"ключ","message":"needs key"}]}`,
		"docs/src/index.json": `{"title":"Index","key":"ключ","body":["Это новыи\u0306 текст.","Слово новыи\u0306 новый подряд."]}`,
	})
	_, out, _ := do("validate", "--format", "json")
	var ds []struct {
		Rule   string
		Column int
	}
	if err := json.Unmarshal(diagnosticsOf(out), &ds); err != nil {
		t.Fatal(err)
	}
	at := map[string][]int{}
	for _, x := range ds {
		at[x.Rule] = append(at[x.Rule], x.Column)
	}
	// the custom rule hits the same three places as the built-in DOC353 word list
	if got, want := fmt.Sprint(at["custom/no-new"]), fmt.Sprint(at["style/timeless-words"]); got != want || len(at["custom/no-new"]) != 3 {
		t.Errorf("custom/no-new at %s, DOC353 at %s", got, want)
	}
	if len(at["custom/needs-key"]) != 0 || len(at["text/repeated-word"]) != 1 {
		t.Errorf("want one DOC313 and no custom/needs-key: %v", at)
	}
}

func TestPlaceholdersAndNFC(t *testing.T) {
	project(t, map[string]string{
		"docs/src/index.json": `{"title":"Index","body":["—","Value: N/A.","Это новыи\u0306 текст."]}`,
	})
	_, out, _ := do("validate", "--format", "json")
	d := string(diagnosticsOf(out))
	for _, want := range []string{`placeholder \"—\"`, `placeholder \"N/A\"`, `"DOC353"`} {
		if !strings.Contains(d, want) {
			t.Errorf("lacks %s:\n%s", want, d)
		}
	}
}
