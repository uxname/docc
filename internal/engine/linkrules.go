package engine

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"

	"golang.org/x/text/unicode/norm"

	"github.com/santiment/docc/internal/loader"
)

// Fingerprint is the first 8 hex digits of SHA-256 of the canonical JSON of a
// document without docIgnore and notes.
func Fingerprint(root *loader.Node) string {
	var b strings.Builder
	canonical(&b, root, fingerprintSkip)
	sum := sha256.Sum256([]byte(b.String()))
	return hex.EncodeToString(sum[:])[:8]
}

var fingerprintSkip = map[string]bool{"docIgnore": true, "notes": true}

// canonical writes n as JSON with sorted keys and NFC strings; skip names the
// keys dropped at the top level only.
func canonical(b *strings.Builder, n *loader.Node, skip map[string]bool) {
	switch n.Kind {
	case loader.Null, loader.Bool, loader.Number:
		b.WriteString(n.Str)
	case loader.String:
		quote(b, n.Str)
	case loader.Array:
		b.WriteByte('[')
		for i, it := range n.Items {
			if i > 0 {
				b.WriteByte(',')
			}
			canonical(b, it, nil)
		}
		b.WriteByte(']')
	case loader.Object:
		type kv struct {
			k string
			v *loader.Node
		}
		var items []kv
		seen := map[string]bool{}
		for _, m := range n.Members {
			if seen[m.Key] {
				continue
			}
			seen[m.Key] = true
			if skip[m.Key] {
				continue
			}
			items = append(items, kv{m.Key, m.Value})
		}
		sort.Slice(items, func(i, j int) bool { return items[i].k < items[j].k })
		b.WriteByte('{')
		for i, it := range items {
			if i > 0 {
				b.WriteByte(',')
			}
			quote(b, it.k)
			b.WriteByte(':')
			canonical(b, it.v, nil)
		}
		b.WriteByte('}')
	}
}

// quote writes a JSON string with minimal escaping: quote, backslash, and
// control characters as \u00XX.
func quote(b *strings.Builder, s string) {
	b.WriteByte('"')
	for _, r := range norm.NFC.String(s) {
		switch {
		case r == '"':
			b.WriteString(`\"`)
		case r == '\\':
			b.WriteString(`\\`)
		case r < 0x20:
			fmt.Fprintf(b, "\\u%04x", r)
		default:
			b.WriteRune(r)
		}
	}
	b.WriteByte('"')
}
