package engine

import (
	"fmt"
	"regexp"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/text"
)

// masked returns the string with code blocks and inline code replaced by
// spaces, so offsets stay the same.
func (s *StrNode) masked() string {
	src := s.Node.Str
	b := []byte(src)
	prev := 0
	for _, sp := range s.Scan().Plain {
		for i := prev; i < sp.Start; i++ {
			if b[i] != '\n' {
				b[i] = ' '
			}
		}
		prev = sp.End
	}
	for i := prev; i < len(b); i++ {
		if b[i] != '\n' {
			b[i] = ' '
		}
	}
	return string(b)
}

var (
	dateDMY  = regexp.MustCompile(`\d{1,2}\.\d{1,2}\.\d{4}`)
	dateSlsh = regexp.MustCompile(`\d{1,2}/\d{1,2}/\d{2}`)
)

// checkStyle runs DOC307, DOC351 and DOC355.
func (p *Project) checkStyle() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		res := p.Cfg.For(d.Rel)
		on := func(code string) bool { return res.Rule(code).Level != diag.Off }
		strs := d.Strings()
		maxWords := 30
		if st := res.Rule("DOC351").Params; st.HasMax {
			maxWords = st.Max
		}
		for i := range strs {
			sn := &strs[i]
			if !sn.IsText() {
				continue
			}
			if on("DOC307") {
				s := sn.Node.Str
				for _, sp := range sn.Scan().Plain {
					seg := s[sp.Start:sp.End]
					for _, m := range dateDMY.FindAllStringIndex(seg, -1) {
						if boundaryOK(seg, m[0], m[1], "./") {
							a, b := d.Range(sn.Node, sp.Start+m[0], sp.Start+m[1])
							p.emit(d, "DOC307", a, b, fmt.Sprintf("date %q is not in YYYY-MM-DD form", seg[m[0]:m[1]]), "Write the date as YYYY-MM-DD.")
						}
					}
					for _, m := range dateSlsh.FindAllStringIndex(seg, -1) {
						if boundaryOK(seg, m[0], m[1], "/") {
							a, b := d.Range(sn.Node, sp.Start+m[0], sp.Start+m[1])
							p.emit(d, "DOC307", a, b, fmt.Sprintf("date %q is not in YYYY-MM-DD form", seg[m[0]:m[1]]), "Write the date as YYYY-MM-DD.")
						}
					}
				}
			}
			if on("DOC351") {
				m := sn.masked()
				for _, sp := range text.Sentences(m) {
					if n := text.CountWords(m[sp.Start:sp.End]); n > maxWords {
						a, b := d.Range(sn.Node, sp.Start, sp.End)
						p.emit(d, "DOC351", a, b, fmt.Sprintf("the sentence has %d words, the limit is %d", n, maxWords), "Split the sentence into shorter ones.")
					}
				}
			}
		}
		if on("DOC355") {
			p.acronyms(d, strs, res.Rule("DOC355").Params.Allow)
		}
	}
}

// boundaryOK checks that a match is not glued to more digits: a digit next
// to it, or a separator followed by a digit (1.2.2026.5, 10/10/101). A
// separator that ends a sentence is fine.
func boundaryOK(s string, a, b int, seps string) bool {
	isDigit := func(c byte) bool { return c >= '0' && c <= '9' }
	if a > 0 {
		if isDigit(s[a-1]) || (a > 1 && strings.IndexByte(seps, s[a-1]) >= 0 && isDigit(s[a-2])) {
			return false
		}
	}
	if b < len(s) {
		if isDigit(s[b]) || (b+1 < len(s) && strings.IndexByte(seps, s[b]) >= 0 && isDigit(s[b+1])) {
			return false
		}
	}
	return true
}

// isAcronym reports a word made only of capital letters, two or more of them
// (a rule code such as DOC101 is not an acronym).
func isAcronym(w string) bool {
	if utf8.RuneCountInString(w) < 2 {
		return false
	}
	for _, r := range w {
		if !unicode.IsUpper(r) {
			return false
		}
	}
	return true
}

// acronyms implements DOC355: the first use of an acronym that is not
// explained anywhere in the document.
func (p *Project) acronyms(d *Doc, strs []StrNode, allow []string) {
	type use struct {
		w    string
		a, b int
		sn   *StrNode
		def  bool
	}
	var uses []use
	defined := map[string]bool{}
	for _, a := range allow {
		defined[strings.ToUpper(strings.TrimSpace(a))] = true
	}
	for i := range strs {
		sn := &strs[i]
		if !sn.IsText() {
			continue
		}
		s := sn.Node.Str
		for _, w := range text.Words(s) {
			if !isAcronym(w.Text) || !sn.Scan().InPlain(w.Start) {
				continue
			}
			key := strings.ToUpper(w.Text)
			// "ACR (explanation)" or "explanation (ACR)"
			after := strings.TrimLeft(s[w.End:], " ")
			wrapped := w.Start > 0 && s[w.Start-1] == '(' && w.End < len(s) && s[w.End] == ')'
			if strings.HasPrefix(after, "(") || wrapped {
				defined[key] = true
				continue
			}
			uses = append(uses, use{key, w.Start, w.End, sn, false})
		}
	}
	reported := map[string]bool{}
	for _, u := range uses {
		if defined[u.w] || reported[u.w] {
			continue
		}
		reported[u.w] = true
		a, b := d.Range(u.sn.Node, u.a, u.b)
		p.emit(d, "DOC355", a, b, fmt.Sprintf("the acronym %q is used without an explanation in this document", u.w),
			fmt.Sprintf("Spell it out once, for example \"... (%s)\" after the full name.", u.w))
	}
}

// Title rules ---------------------------------------------------------------

func normTitle(s string) string { return strings.Join(strings.Fields(strings.ToLower(s)), " ") }

type titled struct {
	d     *Doc
	node  *loader.Node
	norm  string
	group string
}

func (p *Project) titledDocs() []titled {
	var out []titled
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) {
			continue
		}
		t := d.Root.Get("title")
		if t == nil || t.Kind != loader.String || strings.TrimSpace(t.Str) == "" {
			continue
		}
		g := ""
		if k := d.Root.Get("kind"); k != nil && k.Kind == loader.String {
			g = k.Str
		}
		out = append(out, titled{d, t, normTitle(t.Str), g})
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].d.ID < out[j].d.ID })
	return out
}

// checkTitles runs DOC305 and DOC306.
func (p *Project) checkTitles() {
	if p.Off("DOC305") && p.Off("DOC306") {
		return
	}
	docs := p.titledDocs()
	groups := map[string][]titled{}
	for _, t := range docs {
		groups[t.group] = append(groups[t.group], t)
	}
	names := make([]string, 0, len(groups))
	for g := range groups {
		names = append(names, g)
	}
	sort.Strings(names)
	for _, g := range names {
		list := groups[g]
		first := map[string]titled{}
		for _, t := range list {
			if f, ok := first[t.norm]; ok {
				p.emit(t.d, "DOC305", t.node.Start, t.node.End,
					fmt.Sprintf("the title %q is already used by %q", t.node.Str, f.d.ID),
					"Give each document of the same kind a distinct title.")
			} else {
				first[t.norm] = t
			}
		}
		p.similarTitles(list)
	}
}

func trigrams(s string) map[string]bool {
	r := []rune(s)
	out := map[string]bool{}
	if len(r) < 3 {
		out[s] = true
		return out
	}
	for i := 0; i+3 <= len(r); i++ {
		out[string(r[i:i+3])] = true
	}
	return out
}

func jaccard(a, b map[string]bool) float64 {
	inter := 0
	for k := range a {
		if b[k] {
			inter++
		}
	}
	return float64(inter) / float64(len(a)+len(b)-inter)
}

func (p *Project) similarTitles(list []titled) {
	if p.Off("DOC306") || len(list) < 2 {
		return
	}
	grams := make([]map[string]bool, len(list))
	for i, t := range list {
		grams[i] = trigrams(t.norm)
	}
	for j := 1; j < len(list); j++ { // j has the larger id
		b := list[j]
		st := p.Cfg.For(b.d.Rel).Rule("DOC306")
		if st.Level == diag.Off {
			continue
		}
		threshold := 0.85
		if st.Params.HasThreshold {
			threshold = st.Params.Threshold
		}
		allowed := map[string]bool{}
		for _, a := range st.Params.Allow {
			allowed[normTitle(a)] = true
		}
		for i := 0; i < j; i++ {
			a := list[i]
			if a.norm == b.norm || allowed[a.norm] || allowed[b.norm] {
				continue // equal titles are DOC305
			}
			lo, hi := len(grams[i]), len(grams[j])
			if lo > hi {
				lo, hi = hi, lo
			}
			if float64(lo)/float64(hi) < threshold {
				continue
			}
			if sim := jaccard(grams[i], grams[j]); sim >= threshold {
				p.emit(b.d, "DOC306", b.node.Start, b.node.End,
					fmt.Sprintf("the title %q is almost the same as %q of %q (similarity %.2f)", b.node.Str, a.node.Str, a.d.ID, sim),
					"Merge the documents or make the titles clearly different.")
			}
		}
	}
}
