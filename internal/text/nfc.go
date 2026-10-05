package text

import "golang.org/x/text/unicode/norm"

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
