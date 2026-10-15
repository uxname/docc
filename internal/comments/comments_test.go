package comments

import (
	"encoding/base64"
	"strings"
	"testing"
)

// The text copied from the page may have the marker quoted in a comment, and
// a messenger may fold the base64 into lines.
func TestParseExportCopiedText(t *testing.T) {
	js := `{"docc":"comments","version":1,"comments":[{"id":"c1","doc":"d","text":"the docc-comments: line"}]}`
	b := base64.StdEncoding.EncodeToString([]byte(js))
	text := "1. D — «q»\nthe docc-comments: line is damaged\n\nFor the team:\n" + Marker + b + "\n"
	if items, err := ParseExport("t", []byte(text)); err != nil || len(items) != 1 {
		t.Fatalf("quoted marker: %v %v", items, err)
	}
	var folded []string
	for len(b) > 20 {
		folded, b = append(folded, b[:20]), b[20:]
	}
	folded = append(folded, b)
	if items, err := ParseExport("t", []byte("x\n"+Marker+strings.Join(folded, "\n")+"\n")); err != nil || len(items) != 1 {
		t.Fatalf("folded base64: %v %v", items, err)
	}
}
