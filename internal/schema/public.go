package schema

import (
	_ "embed"
	"encoding/json"

	"github.com/santiment/docc/internal/rules"
)

//go:embed config.json
var configSchema []byte

// Document returns the document meta-schema printed by `docc schema document`.
func Document() []byte { return DocumentSchema }

// Config returns the docc.json schema printed by `docc schema config`. The
// allowed keys of the rules sections are taken from the rule registry.
func Config() []byte {
	var m map[string]any
	if err := json.Unmarshal(configSchema, &m); err != nil {
		panic(err)
	}
	var names []any
	for _, r := range rules.All() {
		names = append(names, r.Code)
		if r.Code != "DOC800" {
			names = append(names, r.Name)
		}
	}
	for _, g := range rules.Groups {
		names = append(names, g)
	}
	keys := map[string]any{"anyOf": []any{
		map[string]any{"enum": names},
		map[string]any{"pattern": "^custom/[a-z0-9-]+$"},
	}}
	props := m["properties"].(map[string]any)
	props["rules"].(map[string]any)["propertyNames"] = keys
	ov := props["overrides"].(map[string]any)["items"].(map[string]any)["properties"].(map[string]any)
	ov["rules"].(map[string]any)["propertyNames"] = keys
	b, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		panic(err)
	}
	return append(b, '\n')
}
