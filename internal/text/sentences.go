package text

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

// abbreviations never end a sentence (compared without case).
var abbreviations = []string{"т.д.", "т.п.", "т.е.", "г.", "п.", "рис.", "см.", "e.g.", "i.e."}

type Span struct{ Start, End int }

// Sentences splits s into sentences. A boundary is '.', '!', '?' or '…'
// followed by whitespace and an uppercase letter, or by the end of the text.
// Dots inside numbers and the listed abbreviations are not boundaries.
// Returned spans are trimmed of surrounding whitespace.
func Sentences(s string) []Span {
	var out []Span
	start := 0
	add := func(a, b int) {
		for a < b && isSpace(s[a]) {
			a++
		}
		for b > a && isSpace(s[b-1]) {
			b--
		}
		if a < b {
			out = append(out, Span{a, b})
		}
	}
	for i := 0; i < len(s); {
		r, n := utf8.DecodeRuneInString(s[i:])
		if r != '.' && r != '!' && r != '?' && r != '…' {
			i += n
			continue
		}
		j := i + n
		for j < len(s) && (s[j] == '.' || s[j] == '!' || s[j] == '?') {
			j++ // "?!", "..."
		}
		k := j
		for k < len(s) && isSpace(s[k]) {
			k++
		}
		boundary := k >= len(s) || (k > j && startsUpper(s[k:]))
		if boundary && r == '.' && abbreviationBefore(s[:j]) {
			boundary = false
		}
		if boundary {
			add(start, j)
			start = j
		}
		i = j
	}
	add(start, len(s))
	return out
}

func isSpace(b byte) bool { return b == ' ' || b == '\n' || b == '\t' || b == '\r' }

func startsUpper(s string) bool {
	r, _ := utf8.DecodeRuneInString(s)
	return unicode.IsUpper(r)
}

// abbreviationBefore reports whether s (ending with a dot) ends with one of
// the abbreviations as a separate token.
func abbreviationBefore(s string) bool {
	low := strings.ToLower(s)
	for _, a := range abbreviations {
		if strings.HasSuffix(low, a) {
			before := low[:len(low)-len(a)]
			if before == "" {
				return true
			}
			if r, _ := utf8.DecodeLastRuneInString(before); !unicode.IsLetter(r) && !unicode.IsDigit(r) {
				return true
			}
		}
	}
	return false
}

// CountWords returns the number of words in s.
func CountWords(s string) int { return len(Words(s)) }
