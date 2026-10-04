package loader

import (
	"fmt"
	"strings"
	"testing"
)

func synthDoc(i int) []byte {
	para := strings.Repeat("Система должна обеспечивать устойчивую работу при нагрузке. ", 6)
	var b strings.Builder
	fmt.Fprintf(&b, "{\n  \"title\": \"Документ %d\",\n  \"kind\": \"note\",\n  \"body\": [\n", i)
	n := 5 + i%12 // 5..16 paragraphs of ~0.36 KB of Cyrillic (2 bytes/rune) => 5..20 KB
	for k := 0; k < n; k++ {
		if k > 0 {
			b.WriteString(",\n")
		}
		fmt.Fprintf(&b, "    %q", para)
	}
	b.WriteString("\n  ]\n}\n")
	return []byte(b.String())
}

func BenchmarkParse1000(b *testing.B) {
	docs := make([][]byte, 1000)
	total := 0
	for i := range docs {
		docs[i] = synthDoc(i)
		total += len(docs[i])
	}
	b.SetBytes(int64(total))
	b.ResetTimer()
	for n := 0; n < b.N; n++ {
		for _, d := range docs {
			if r := Parse(d); r.Err != nil {
				b.Fatal(r.Err)
			}
		}
	}
}
