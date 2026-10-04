package mdscan

import (
	"regexp"
	"strings"
)

var (
	mdLink  = regexp.MustCompile(`\[([^\[\]\n]*)\]\(([^()\s]*)\)`)
	bareURL = regexp.MustCompile(`https?://[^\s<>\[\]()]+`)
)

// Link is a Markdown link [text](url).
type Link struct {
	Span      // whole link
	Text      string
	TextStart int
	TextEnd   int
	URL       string
}

// Links returns Markdown links outside code.
func (sc *Scan) Links() []Link {
	var out []Link
	for _, sp := range sc.Plain {
		t := sc.S[sp.Start:sp.End]
		for _, m := range mdLink.FindAllStringSubmatchIndex(t, -1) {
			out = append(out, Link{
				Span:      Span{sp.Start + m[0], sp.Start + m[1]},
				Text:      t[m[2]:m[3]],
				TextStart: sp.Start + m[2],
				TextEnd:   sp.Start + m[3],
				URL:       t[m[4]:m[5]],
			})
		}
	}
	return out
}

// BareURLs returns http(s) addresses outside code that are not part of a
// Markdown link.
func (sc *Scan) BareURLs() []Span {
	links := sc.Links()
	var out []Span
	for _, sp := range sc.Plain {
		t := sc.S[sp.Start:sp.End]
		for _, m := range bareURL.FindAllStringIndex(t, -1) {
			a, b := sp.Start+m[0], sp.Start+m[1]
			for b > a && strings.ContainsRune(".,;:!?'\"", rune(sc.S[b-1])) {
				b--
			}
			inside := false
			for _, l := range links {
				if a >= l.Start && a < l.End {
					inside = true
				}
			}
			if !inside {
				out = append(out, Span{a, b})
			}
		}
	}
	return out
}
