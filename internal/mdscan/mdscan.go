// Package mdscan is a small scanner for the Markdown that lives inside JSON
// string values: code blocks, inline code, [[refs]], {{includes}}, headings,
// raw HTML tags and links. All offsets are byte offsets in the scanned string.
package mdscan

import (
	"regexp"
	"strings"
)

type Span struct{ Start, End int }

// Result of scanning one string value.
type Scan struct {
	S             string
	Plain         []Span // text outside fenced blocks and inline code
	UnclosedFence int    // offset of an unclosed opening fence, or -1
}

// New scans s.
func New(s string) *Scan {
	sc := &Scan{S: s, UnclosedFence: -1}
	var chunkStart = -1
	flush := func(end int) {
		if chunkStart >= 0 && end > chunkStart {
			sc.inline(chunkStart, end)
		}
		chunkStart = -1
	}
	var fenceCh byte
	fenceLen := 0
	pos := 0
	for pos <= len(s) {
		end := strings.IndexByte(s[pos:], '\n')
		next := len(s) + 1
		lineEnd := len(s)
		if end >= 0 {
			lineEnd = pos + end
			next = lineEnd + 1
		}
		line := s[pos:lineEnd]
		if fenceLen > 0 {
			if ch, n, rest := fenceRun(line); n >= fenceLen && ch == fenceCh && strings.TrimSpace(rest) == "" {
				fenceLen = 0
			}
		} else if ch, n, _ := fenceRun(line); n >= 3 {
			flush(pos)
			fenceCh, fenceLen = ch, n
			sc.UnclosedFence = pos
		} else if chunkStart < 0 {
			chunkStart = pos
		}
		pos = next
	}
	if fenceLen > 0 {
		// unclosed: UnclosedFence stays at the opener, the rest is code
		return sc
	}
	sc.UnclosedFence = -1
	flush(len(s))
	return sc
}

// fenceRun detects a fence line: up to 3 spaces, then 3+ of ` or ~.
func fenceRun(line string) (ch byte, n int, rest string) {
	i := 0
	for i < len(line) && i < 3 && line[i] == ' ' {
		i++
	}
	if i >= len(line) || (line[i] != '`' && line[i] != '~') {
		return 0, 0, ""
	}
	ch = line[i]
	j := i
	for j < len(line) && line[j] == ch {
		j++
	}
	if ch == '`' && strings.Contains(line[j:], "`") {
		return 0, 0, "" // inline code, not a fence
	}
	return ch, j - i, line[j:]
}

// inline splits chunk [a,b) into plain spans, cutting out inline code.
func (sc *Scan) inline(a, b int) {
	s := sc.S
	start := a
	i := a
	for i < b {
		switch s[i] {
		case '\\':
			i += 2
			continue
		case '`':
			j := i
			for j < b && s[j] == '`' {
				j++
			}
			n := j - i
			// find closing run of exactly n backticks
			k := j
			closed := -1
			for k < b {
				if s[k] != '`' {
					k++
					continue
				}
				m := k
				for m < b && s[m] == '`' {
					m++
				}
				if m-k == n {
					closed = m
					break
				}
				k = m
			}
			if closed < 0 {
				i = j
				continue
			}
			if i > start {
				sc.Plain = append(sc.Plain, Span{start, i})
			}
			start = closed
			i = closed
			continue
		}
		i++
	}
	if i > b {
		i = b
	}
	if start < b {
		sc.Plain = append(sc.Plain, Span{start, b})
	}
}

// Ref is a [[...]] or {{...}} occurrence: Start/End include the brackets.
type Ref struct {
	Start, End int
	Inner      string
}

func (sc *Scan) delimited(open, close string) []Ref {
	var out []Ref
	for _, sp := range sc.Plain {
		t := sc.S[sp.Start:sp.End]
		for i := 0; i < len(t); {
			a := strings.Index(t[i:], open)
			if a < 0 {
				break
			}
			a += i
			z := strings.Index(t[a+len(open):], close)
			if z < 0 {
				break
			}
			inner := t[a+len(open) : a+len(open)+z]
			if strings.ContainsAny(inner, "[]{}\n") {
				i = a + 1
				continue
			}
			out = append(out, Ref{sp.Start + a, sp.Start + a + len(open) + z + len(close), inner})
			i = a + len(open) + z + len(close)
		}
	}
	return out
}

// Refs returns [[...]] occurrences outside code.
func (sc *Scan) Refs() []Ref { return sc.delimited("[[", "]]") }

// Includes returns {{...}} occurrences outside code.
func (sc *Scan) Includes() []Ref { return sc.delimited("{{", "}}") }

// InPlain reports whether offset lies in plain (non-code) text.
func (sc *Scan) InPlain(off int) bool {
	for _, sp := range sc.Plain {
		if off >= sp.Start && off < sp.End {
			return true
		}
	}
	return false
}

// Headings returns the offsets of lines that start with 1-6 '#' and a space,
// outside code blocks.
func (sc *Scan) Headings() []Span {
	var out []Span
	for _, sp := range sc.Plain {
		for off := sp.Start; off < sp.End; {
			// only line starts count
			if off == 0 || sc.S[off-1] == '\n' {
				n := 0
				for off+n < sp.End && sc.S[off+n] == '#' {
					n++
				}
				if n >= 1 && n <= 6 && off+n < len(sc.S) && sc.S[off+n] == ' ' {
					end := strings.IndexByte(sc.S[off:], '\n')
					if end < 0 {
						end = len(sc.S) - off
					}
					out = append(out, Span{off, off + end})
				}
			}
			nl := strings.IndexByte(sc.S[off:sp.End], '\n')
			if nl < 0 {
				break
			}
			off += nl + 1
		}
	}
	return out
}

var htmlOpen = regexp.MustCompile(`<([A-Za-z][A-Za-z0-9]*)(?:\s[^<>]*)?/?>`)

var htmlElements = map[string]bool{}

func init() {
	for n := range strings.FieldsSeq(`a abbr address area article aside audio b base bdi bdo blockquote body br button canvas caption cite code col colgroup data datalist dd del details dfn dialog div dl dt em embed fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 head header hgroup hr html i iframe img input ins kbd label legend li link main map mark menu meta meter nav noscript object ol optgroup option output p param picture pre progress q rp rt ruby s samp script search section select slot small source span strong style sub summary sup table tbody td template textarea tfoot th thead time title tr track u ul var video wbr`) {
		htmlElements[n] = true
	}
}

// Tag is a raw HTML opening tag.
type Tag struct {
	Span
	Name string
}

// HTMLTags returns opening tags with known HTML element names outside code.
func (sc *Scan) HTMLTags() []Tag {
	var out []Tag
	for _, sp := range sc.Plain {
		t := sc.S[sp.Start:sp.End]
		for _, m := range htmlOpen.FindAllStringSubmatchIndex(t, -1) {
			name := strings.ToLower(t[m[2]:m[3]])
			if htmlElements[name] {
				out = append(out, Tag{Span{sp.Start + m[0], sp.Start + m[1]}, name})
			}
		}
	}
	return out
}
