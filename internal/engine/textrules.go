package engine

import (
	"fmt"
	"regexp"
	"strings"
	"unicode"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/loader"
	"github.com/santiment/docc/internal/text"
)

// matcher returns a compiled word list, cached by content.
func (p *Project) matcher(builtin, extra []string) *text.Matcher {
	key := strings.Join(builtin, "\x00") + "\x01" + strings.Join(extra, "\x00")
	if p.matchers == nil {
		p.matchers = map[string]*text.Matcher{}
	}
	if m, ok := p.matchers[key]; ok {
		return m
	}
	m := text.NewMatcher(append(append([]string{}, builtin...), extra...))
	p.matchers[key] = m
	return m
}

// listRule describes a rule driven by a word list.
type listRule struct {
	code    string
	builtin []string
	msg     string // format with the matched text
	hint    string
}

var listRules = []listRule{
	{"DOC301", text.Forbidden, "forbidden word or phrase %q", "Replace it with a specific, measurable statement."},
	{"DOC303", text.Placeholders, "placeholder %q is left in the text", "Write the real text or remove the placeholder."},
	{"DOC352", text.Hedging, "hedging word %q", "State the fact plainly, or say exactly when it holds."},
	{"DOC353", text.Timeless, "time-bound word %q", "Say what is true regardless of the moment, or give a date."},
}

// checkText runs the word-based rules on every prose string.
func (p *Project) checkText() {
	for _, d := range p.Docs {
		if !d.Parsed() {
			continue
		}
		res := p.Cfg.Resolve()
		on := func(code string) bool { return res.Rule(code).Level != diag.Off }
		if p.Cfg.Numbering && on("DOC314") {
			p.manualNumbers(d)
		}
		strs := d.Strings()
		for i := range strs {
			sn := &strs[i]
			if !sn.IsText() {
				continue
			}
			s := sn.Node.Str
			if on("DOC304") && sn.Field != "title" && strings.TrimSpace(s) == "" {
				p.emit(d, "DOC304", sn.Node.Start, sn.Node.End, "the string is empty", "Fill it in or remove the field.")
				continue
			}
			for _, lr := range listRules {
				if on(lr.code) {
					p.listHits(d, sn, lr, res.Rule(lr.code))
				}
			}
			if on("DOC313") {
				p.repeatedWords(d, sn, res.Rule("DOC313"))
			}
		}
	}
}

func (p *Project) listHits(d *Doc, sn *StrNode, lr listRule, st config.Setting) {
	var m, allow *text.Matcher
	if lr.code == "DOC303" {
		m = p.matcher(lr.builtin, nil)
	} else {
		m = p.matcher(lr.builtin, st.Params.Words)
	}
	if len(st.Params.Allow) > 0 {
		allow = p.matcher(nil, st.Params.Allow)
	}
	s := sn.Node.Str
	for _, sp := range sn.Scan().Prose() {
		seg := s[sp.Start:sp.End]
		hits := m.Find(seg)
		if allow != nil {
			hits = text.Without(hits, allow.Find(seg))
		}
		for _, h := range hits {
			a, b := d.Range(sn.Node, sp.Start+h.Start, sp.Start+h.End)
			p.emit(d, lr.code, a, b, fmt.Sprintf(lr.msg, seg[h.Start:h.End]), lr.hint)
		}
	}
}

// repeatedWords implements DOC313.
func (p *Project) repeatedWords(d *Doc, sn *StrNode, st config.Setting) {
	var allow *text.Matcher
	if len(st.Params.Allow) > 0 {
		allow = p.matcher(nil, st.Params.Allow)
	}
	s := sn.Node.Str
	for _, sp := range sn.Scan().Prose() {
		seg := s[sp.Start:sp.End]
		ws := text.Words(seg)
		for i := 1; i < len(ws); i++ {
			a, b := ws[i-1], ws[i]
			if !strings.EqualFold(a.Text, b.Text) || !hasLetter(a.Text) {
				continue
			}
			gap := seg[a.End:b.Start]
			if gap == "" || strings.Trim(gap, " \t") != "" {
				continue
			}
			if allow != nil && len(allow.Find(seg[a.Start:b.End])) > 0 {
				continue
			}
			ra, rb := d.Range(sn.Node, sp.Start+a.Start, sp.Start+b.End)
			p.emit(d, "DOC313", ra, rb, fmt.Sprintf("the word %q is repeated", a.Text), "Remove the repeated word.")
		}
	}
}

func hasLetter(s string) bool {
	for _, r := range s {
		if unicode.IsLetter(r) {
			return true
		}
	}
	return false
}

var manualNumber = regexp.MustCompile(`^\d+(\.\d+)*\.?\s`)

// manualNumbers implements DOC314: with numbering on, a title or a section
// key must not carry its own number. Field text is not checked.
func (p *Project) manualNumbers(d *Doc) {
	const msg, hint = "the number is added by docc; remove it from the title", "Set the position with the order field; docc numbers pages and sections itself."
	if t := d.Root.Get("title"); t != nil && t.Kind == loader.String && manualNumber.MatchString(t.Str) {
		p.emit(d, "DOC314", t.Start, t.End, msg, hint)
	}
	var walk func(n *loader.Node)
	walk = func(n *loader.Node) {
		for _, m := range n.Members {
			if m.Key == SectionsKey {
				continue
			}
			if manualNumber.MatchString(m.Key) {
				p.emit(d, "DOC314", m.KeyStart, m.KeyEnd, msg, hint)
			}
			if IsSections(m.Value) {
				walk(m.Value)
			}
		}
	}
	for _, m := range d.Root.Members {
		if !ServiceFields[m.Key] && IsSections(m.Value) {
			walk(m.Value)
		}
	}
}
