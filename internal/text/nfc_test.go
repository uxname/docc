package text

import (
	"testing"

	"golang.org/x/text/unicode/norm"
)

// A stable rune must be NFC alone and must not compose with a mark after it
// or with a letter before it, otherwise the fast path of IsNFC is wrong.
func TestStableRunes(t *testing.T) {
	for r := rune(0); r < 0x2100; r++ {
		if !stableRune(r) || r < 0x20 {
			continue
		}
		s := string(r)
		if !norm.NFC.IsNormalString(s) {
			t.Fatalf("%U is not NFC", r)
		}
		if norm.NFC.String("a"+s) != "a"+s || norm.NFC.String("и"+s) != "и"+s {
			t.Fatalf("%U composes with the letter before it", r)
		}
		if p := norm.NFC.Properties([]byte(s)); p.CCC() != 0 {
			t.Fatalf("%U is a combining mark", r)
		}
	}
	if IsNFC("й") || NFC("й") != "й" || !IsNFC("Заголовок — «текст»") {
		t.Fatal("IsNFC/NFC")
	}
}
