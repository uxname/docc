package text

import (
	"sort"
	"strings"

	"golang.org/x/text/unicode/norm"
)

// IsNFC reports whether s is in NFC. Text made only of runes that are always
// NFC on their own and never combine with a neighbour (ASCII and Latin-1,
// Cyrillic letters, dashes and quotes) is answered without the norm tables,
// which are slow on long Cyrillic text.
func IsNFC(s string) bool {
	for _, r := range s {
		if !stableRune(r) {
			return norm.NFC.IsNormalString(s)
		}
	}
	return true
}

// NFC returns s in NFC.
func NFC(s string) string {
	if IsNFC(s) {
		return s
	}
	return norm.NFC.String(s)
}

func stableRune(r rune) bool {
	return r < 0x300 || (r >= 0x400 && r <= 0x482) || (r >= 0x48a && r <= 0x4ff) || (r >= 0x2010 && r <= 0x2027)
}

// NFCMap returns s in NFC and a function that maps a byte range of the NFC
// text back to the range of s that holds it, so a match found in NFC is
// reported at the right place in the source.
func NFCMap(s string) (string, func(a, b int) (int, int)) {
	if IsNFC(s) {
		return s, func(a, b int) (int, int) { return a, b }
	}
	// segN[i] and segO[i] are the starts of the i-th normalization segment in the NFC text and in s
	var b strings.Builder
	var segN, segO []int
	var it norm.Iter
	it.InitString(norm.NFC, s)
	for o := 0; !it.Done(); o = it.Pos() {
		segN, segO = append(segN, b.Len()), append(segO, o)
		b.Write(it.Next())
	}
	segN, segO = append(segN, b.Len()), append(segO, len(s))
	return b.String(), func(start, end int) (int, int) {
		k := sort.SearchInts(segN, start+1) - 1          // the segment that holds the start
		return segO[k], segO[sort.SearchInts(segN, end)] // the end of the segment that holds the end
	}
}
