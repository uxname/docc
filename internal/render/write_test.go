package render

import (
	"testing"

	"github.com/uxname/docc/internal/config"
	"github.com/uxname/docc/internal/engine"
)

func TestSingleFileRefusesSourcesAndSchemas(t *testing.T) {
	p := &engine.Project{Cfg: &config.Config{Root: t.TempDir(), File: "docc.json", Src: "docs/src", Schemas: "docs/schemas"}}
	for _, out := range []string{"docs/src/out", "docs/schemas/out", "docs/src"} {
		if f, err := SingleFile(p, out, ".md"); err == nil && out != "docs/src" {
			t.Errorf("SingleFile(%q) = %q, want a refusal", out, f)
		}
	}
	if _, err := SingleFile(p, "docs/out", ".md"); err != nil {
		t.Errorf("docs/out must be allowed: %v", err)
	}
}
