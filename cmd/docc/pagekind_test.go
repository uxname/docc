package main

import (
	"strings"

	"github.com/santiment/docc/internal/loader"
)

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
