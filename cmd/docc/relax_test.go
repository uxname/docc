package main

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/santiment/docc/internal/loader"
)

// relax switches DOC209 off in the docc.json of a test project (and creates
// the file when there is none), unless the project sets it itself. Most
// fixtures have no reviewed entries, and the rule is on by default. The edit keeps the lines above it where they were, because
// some cases assert positions inside docc.json.
// pageKind gives every document under a src/ folder that parses and has no
// kind the kind "page" (as the last member, so positions above it stay) and
// adds an open docs/schemas/page.json when the project has none. Tests that
// are not about kinds stay short this way.
func pageKind(files map[string]string) {
	added := false
	for name, text := range files {
		if !strings.HasSuffix(name, ".json") || !strings.Contains("/"+name, "/src/") {
			continue
		}
		r := loader.Parse([]byte(text))
		if r.Err != nil || len(r.Dups) > 0 || r.Root.Kind != loader.Object || r.Root.Get("kind") != nil {
			continue
		}
		i := strings.LastIndex(text, "}")
		head := strings.TrimRight(text[:i], " \t\r\n")
		sep := ","
		if strings.HasSuffix(head, "{") {
			sep = ""
		}
		files[name] = head + sep + `"kind":"page"` + text[len(head):]
		added = true
	}
	if _, ok := files["docs/schemas/page.json"]; added && !ok {
		files["docs/schemas/page.json"] = `{"type":"object","additionalProperties":true}`
	}
}

func relax(dir string) {
	name := filepath.Join(dir, "docc.json")
	b, err := os.ReadFile(name)
	if err != nil {
		_ = os.WriteFile(name, []byte(`{ "rules": { "DOC209": "off" } }`+"\n"), 0o644)
		return
	}
	text := string(b)
	var add []string
	for _, code := range []string{"DOC209"} {
		if !strings.Contains(text, `"`+code+`"`) {
			add = append(add, `"`+code+`": "off"`)
		}
	}
	if len(add) == 0 {
		return
	}
	rules := regexp.MustCompile(`"rules"\s*:\s*\{`).FindStringIndex(text)
	switch last := strings.LastIndex(text, "}"); {
	case rules != nil:
		text = text[:rules[1]] + " " + strings.Join(add, ", ") + "," + text[rules[1]:]
	case last >= 0:
		sep := ","
		if strings.HasSuffix(strings.TrimSpace(text[:last]), "{") {
			sep = ""
		}
		text = text[:last] + sep + "\n\"rules\": { " + strings.Join(add, ", ") + " }\n" + text[last:]
	default:
		return // not a JSON object: the case tests a broken config
	}
	_ = os.WriteFile(name, []byte(text), 0o644)
}
