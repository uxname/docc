// Package text holds the word, phrase and sentence logic of the text and style
// rules. Everything works on one string value and returns byte offsets in it.
package text

import (
	"regexp"
	"strings"
	"unicode"
	"unicode/utf8"
)

func isWordRune(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) || r == '_' }

func isJoiner(r rune) bool { return r == '-' || r == '\'' || r == '’' }

// wordBefore reports whether the text just before offset i continues a word.
func wordBefore(s string, i int) bool {
	if i <= 0 {
		return false
	}
	r, n := utf8.DecodeLastRuneInString(s[:i])
	if isWordRune(r) {
		return true
	}
	if isJoiner(r) && i-n > 0 {
		p, _ := utf8.DecodeLastRuneInString(s[:i-n])
		return isWordRune(p)
	}
	return false
}

// wordAfter reports whether the text starting at offset i continues a word.
func wordAfter(s string, i int) bool {
	if i >= len(s) {
		return false
	}
	r, n := utf8.DecodeRuneInString(s[i:])
	if isWordRune(r) {
		return true
	}
	if isJoiner(r) && i+n < len(s) {
		nx, _ := utf8.DecodeRuneInString(s[i+n:])
		return isWordRune(nx)
	}
	return false
}

// Word is a word token.
type Word struct {
	Start, End int
	Text       string
}

// Words splits s into words: runs of letters, digits and '_', with '-' or an
// apostrophe allowed inside a word.
func Words(s string) []Word {
	var out []Word
	i := 0
	for i < len(s) {
		r, n := utf8.DecodeRuneInString(s[i:])
		if !isWordRune(r) {
			i += n
			continue
		}
		j := i + n
		for j < len(s) {
			r2, n2 := utf8.DecodeRuneInString(s[j:])
			if isWordRune(r2) {
				j += n2
				continue
			}
			if isJoiner(r2) && j+n2 < len(s) {
				r3, _ := utf8.DecodeRuneInString(s[j+n2:])
				if isWordRune(r3) {
					j += n2
					continue
				}
			}
			break
		}
		out = append(out, Word{i, j, s[i:j]})
		i = j
	}
	return out
}

// Matcher finds entries of a word list in text. An entry is a word or a phrase
// compared without case; a trailing * on a word makes it a prefix.
type Matcher struct {
	entries []entry
}

type entry struct {
	text       string
	re         *regexp.Regexp
	startBound bool // the entry starts with a word character
	endBound   bool // the entry ends with a word character and no star
}

// NewMatcher compiles word-list entries; empty entries are skipped.
func NewMatcher(list []string) *Matcher {
	m := &Matcher{}
	for _, e := range list {
		e = strings.TrimSpace(e)
		if e == "" {
			continue
		}
		parts := strings.Fields(e)
		var re strings.Builder
		re.WriteString("(?i)")
		for i, p := range parts {
			if i > 0 {
				re.WriteString(`\s+`)
			}
			if strings.HasSuffix(p, "*") {
				re.WriteString(regexp.QuoteMeta(strings.TrimSuffix(p, "*")))
				re.WriteString(`[\p{L}\p{N}_'’-]*`)
			} else {
				re.WriteString(regexp.QuoteMeta(p))
			}
		}
		first, _ := utf8.DecodeRuneInString(e)
		last, _ := utf8.DecodeLastRuneInString(e)
		m.entries = append(m.entries, entry{
			text:       e,
			re:         regexp.MustCompile(re.String()),
			startBound: isWordRune(first),
			endBound:   isWordRune(last),
		})
	}
	return m
}

// Hit is one match of an entry.
type Hit struct {
	Start, End int
	Entry      string
}

// Find returns all matches in s ordered by position.
func (m *Matcher) Find(s string) []Hit {
	var hits []Hit
	for _, e := range m.entries {
		pos := 0
		for pos <= len(s) {
			loc := e.re.FindStringIndex(s[pos:])
			if loc == nil {
				break
			}
			a, b := pos+loc[0], pos+loc[1]
			if (e.startBound && wordBefore(s, a)) || (e.endBound && wordAfter(s, b)) {
				_, n := utf8.DecodeRuneInString(s[a:])
				if n == 0 {
					n = 1
				}
				pos = a + n
				continue
			}
			hits = append(hits, Hit{a, b, e.text})
			if b == a {
				b++
			}
			pos = b
		}
	}
	sortHits(hits)
	out := hits[:0]
	for i, h := range hits {
		if i > 0 && h.Start == hits[i-1].Start && h.End == hits[i-1].End {
			continue // two entries hit the same text
		}
		out = append(out, h)
	}
	return out
}

func sortHits(h []Hit) {
	for i := 1; i < len(h); i++ {
		for j := i; j > 0 && (h[j].Start < h[j-1].Start || (h[j].Start == h[j-1].Start && h[j].End < h[j-1].End)); j-- {
			h[j], h[j-1] = h[j-1], h[j]
		}
	}
}

// Without removes hits that overlap an allow-list match.
func Without(hits []Hit, allow []Hit) []Hit {
	if len(allow) == 0 {
		return hits
	}
	out := hits[:0:0]
	for _, h := range hits {
		skip := false
		for _, a := range allow {
			if h.Start < a.End && a.Start < h.End {
				skip = true
				break
			}
		}
		if !skip {
			out = append(out, h)
		}
	}
	return out
}
