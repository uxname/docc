package main

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

// relax switches DOC109 and DOC209 off in the docc.json of a test project (and
// creates the file when there is none), unless the project sets them itself.
// Most fixtures have no kind schemas and no reviewed entries, and the two rules
// are on by default. The edit keeps the lines above it where they were, because
// some cases assert positions inside docc.json.
func relax(dir string) {
	name := filepath.Join(dir, "docc.json")
	b, err := os.ReadFile(name)
	if err != nil {
		_ = os.WriteFile(name, []byte(`{ "rules": { "DOC109": "off", "DOC209": "off" } }`+"\n"), 0o644)
		return
	}
	text := string(b)
	var add []string
	for _, code := range []string{"DOC109", "DOC209"} {
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
