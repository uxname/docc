// Package loader parses strict JSON into a node tree that keeps byte offsets,
// so diagnostics can point at the exact place in the source file.
package loader

import (
	"bytes"
	"fmt"
	"sort"
	"strconv"
	"unicode/utf8"
)

const (
	MaxFileSize = 5 << 20
	MaxDepth    = 64
)

type Kind uint8

const (
	Null Kind = iota
	Bool
	Number
	String
	Array
	Object
)

type Member struct {
	Key              string
	KeyStart, KeyEnd int
	Value            *Node
}

// Node is one JSON value. Start/End are byte offsets of the token in Source.Data.
// Str holds the decoded string, or the raw text for numbers and booleans.
type Node struct {
	Kind       Kind
	Start, End int
	Str        string
	Items      []*Node
	Members    []Member
}

// Get returns the first member with the given key, or nil.
func (n *Node) Get(key string) *Node {
	if n == nil || n.Kind != Object {
		return nil
	}
	for i := range n.Members {
		if n.Members[i].Key == key {
			return n.Members[i].Value
		}
	}
	return nil
}

type Source struct {
	Data       []byte // without BOM
	BOM        bool
	lineStarts []int
}

type Error struct {
	Offset int
	Msg    string
}

func (e *Error) Error() string { return e.Msg }

// Dup is a repeated key; Start/End cover the repeated key token.
type Dup struct {
	Key        string
	Start, End int
}

type Result struct {
	Src  *Source
	Root *Node
	Err  *Error
	Dups []Dup
}

func newSource(data []byte) *Source {
	s := &Source{}
	if bytes.HasPrefix(data, []byte{0xEF, 0xBB, 0xBF}) {
		s.BOM = true
		data = data[3:]
	}
	s.Data = data
	return s
}

func (s *Source) lines() []int {
	if s.lineStarts == nil {
		s.lineStarts = append(s.lineStarts, 0)
		for i, b := range s.Data {
			if b == '\n' {
				s.lineStarts = append(s.lineStarts, i+1)
			}
		}
	}
	return s.lineStarts
}

// Position returns 1-based line and rune column of a byte offset.
func (s *Source) Position(off int) (line, col int) {
	ls := s.lines()
	if off > len(s.Data) {
		off = len(s.Data)
	}
	if off < 0 {
		off = 0
	}
	i := sort.Search(len(ls), func(i int) bool { return ls[i] > off }) - 1
	return i + 1, utf8.RuneCount(s.Data[ls[i]:off]) + 1
}

// StrOffset maps a byte index of the decoded string value of n to a source
// offset. Bytes produced by one escape sequence map to the start of that escape.
func (s *Source) StrOffset(n *Node, i int) int {
	raw := s.Data[n.Start+1 : n.End-1]
	if bytes.IndexByte(raw, '\\') < 0 {
		if i > len(raw) {
			i = len(raw)
		}
		return n.Start + 1 + i
	}
	d := 0
	for p := 0; p < len(raw); {
		var k, l int
		if raw[p] != '\\' {
			_, l = utf8.DecodeRune(raw[p:])
			k = l
		} else {
			k, l = escapeSize(raw[p:])
		}
		if i < d+k {
			return n.Start + 1 + p
		}
		d += k
		p += l
	}
	return n.End - 1
}

// escapeSize returns decoded byte count and raw length of the escape at b[0]=='\\'.
func escapeSize(b []byte) (decoded, raw int) {
	if len(b) < 2 || b[1] != 'u' {
		return 1, 2
	}
	r, ok := hex4(b[2:])
	if !ok {
		return 1, 2
	}
	if r >= 0xD800 && r < 0xDC00 && len(b) >= 12 && b[6] == '\\' && b[7] == 'u' {
		if r2, ok := hex4(b[8:]); ok && r2 >= 0xDC00 && r2 < 0xE000 {
			return 4, 12
		}
	}
	if r >= 0xD800 && r < 0xE000 {
		return 3, 6
	}
	return utf8.RuneLen(rune(r)), 6
}

func hex4(b []byte) (int, bool) {
	if len(b) < 4 {
		return 0, false
	}
	v := 0
	for _, c := range b[:4] {
		switch {
		case c >= '0' && c <= '9':
			v = v<<4 | int(c-'0')
		case c >= 'a' && c <= 'f':
			v = v<<4 | int(c-'a'+10)
		case c >= 'A' && c <= 'F':
			v = v<<4 | int(c-'A'+10)
		default:
			return 0, false
		}
	}
	return v, true
}

type parser struct {
	d    []byte
	p    int
	dups []Dup
}

type perr struct{ e Error }

func (p *parser) fail(off int, format string, a ...any) {
	panic(perr{Error{off, fmt.Sprintf(format, a...)}})
}

// Parse parses a document file. The root must be an object.
func Parse(data []byte) *Result {
	src := newSource(data)
	r := &Result{Src: src}
	d := src.Data
	if len(d) > MaxFileSize {
		r.Err = &Error{0, "file is larger than 5 MB"}
		return r
	}
	if !utf8.Valid(d) {
		off := 0
		for off < len(d) {
			c, l := utf8.DecodeRune(d[off:])
			if c == utf8.RuneError && l == 1 {
				break
			}
			off += l
		}
		r.Err = &Error{off, "file is not valid UTF-8"}
		return r
	}
	p := &parser{d: d}
	func() {
		defer func() {
			if x := recover(); x != nil {
				if pe, ok := x.(perr); ok {
					r.Err = &pe.e
					return
				}
				panic(x)
			}
		}()
		p.ws()
		if p.p >= len(d) {
			p.fail(p.p, "file is empty")
		}
		root := p.value(0)
		p.ws()
		if p.p < len(d) {
			p.fail(p.p, "unexpected data after the top-level value")
		}
		if root.Kind != Object {
			p.fail(root.Start, "the top-level value must be an object")
		}
		r.Root = root
	}()
	if r.Err == nil {
		r.Dups = p.dups
	}
	return r
}

func (p *parser) ws() {
	for p.p < len(p.d) {
		switch p.d[p.p] {
		case ' ', '\t', '\n', '\r':
			p.p++
		default:
			return
		}
	}
}

func (p *parser) value(depth int) *Node {
	if depth >= MaxDepth {
		p.fail(p.p, "nesting is deeper than %d levels", MaxDepth)
	}
	if p.p >= len(p.d) {
		p.fail(p.p, "unexpected end of file")
	}
	switch c := p.d[p.p]; {
	case c == '{':
		return p.object(depth)
	case c == '[':
		return p.array(depth)
	case c == '"':
		return p.str()
	case c == '-' || (c >= '0' && c <= '9'):
		return p.number()
	case c == 't':
		return p.lit("true", Bool)
	case c == 'f':
		return p.lit("false", Bool)
	case c == 'n':
		return p.lit("null", Null)
	default:
		p.fail(p.p, "unexpected character %q", rune(c))
	}
	return nil
}

func (p *parser) lit(s string, k Kind) *Node {
	if !bytes.HasPrefix(p.d[p.p:], []byte(s)) {
		p.fail(p.p, "invalid literal")
	}
	n := &Node{Kind: k, Start: p.p, End: p.p + len(s), Str: s}
	p.p += len(s)
	return n
}

func (p *parser) number() *Node {
	st := p.p
	if p.d[p.p] == '-' {
		p.p++
	}
	digits := func() int {
		s := p.p
		for p.p < len(p.d) && p.d[p.p] >= '0' && p.d[p.p] <= '9' {
			p.p++
		}
		return p.p - s
	}
	if p.p < len(p.d) && p.d[p.p] == '0' {
		p.p++
	} else if digits() == 0 {
		p.fail(p.p, "invalid number")
	}
	if p.p < len(p.d) && p.d[p.p] == '.' {
		p.p++
		if digits() == 0 {
			p.fail(p.p, "invalid number")
		}
	}
	if p.p < len(p.d) && (p.d[p.p] == 'e' || p.d[p.p] == 'E') {
		p.p++
		if p.p < len(p.d) && (p.d[p.p] == '+' || p.d[p.p] == '-') {
			p.p++
		}
		if digits() == 0 {
			p.fail(p.p, "invalid number")
		}
	}
	return &Node{Kind: Number, Start: st, End: p.p, Str: string(p.d[st:p.p])}
}

func (p *parser) str() *Node {
	st := p.p
	p.p++
	plain := true
	for {
		if p.p >= len(p.d) {
			p.fail(st, "unterminated string")
		}
		c := p.d[p.p]
		if c == '"' {
			break
		}
		if c < 0x20 {
			p.fail(p.p, "control character in string")
		}
		if c == '\\' {
			plain = false
			p.p++
			if p.p >= len(p.d) {
				p.fail(st, "unterminated string")
			}
			switch p.d[p.p] {
			case '"', '\\', '/', 'b', 'f', 'n', 'r', 't':
			case 'u':
				if _, ok := hex4(p.d[p.p+1:]); !ok {
					p.fail(p.p-1, "invalid \\u escape")
				}
				p.p += 4
			default:
				p.fail(p.p-1, "invalid escape")
			}
		}
		p.p++
	}
	end := p.p + 1
	raw := p.d[st+1 : p.p]
	p.p = end
	n := &Node{Kind: String, Start: st, End: end}
	if plain {
		n.Str = string(raw)
	} else {
		n.Str = unescape(raw)
	}
	return n
}

func unescape(raw []byte) string {
	out := make([]byte, 0, len(raw))
	for i := 0; i < len(raw); {
		if raw[i] != '\\' {
			out = append(out, raw[i])
			i++
			continue
		}
		switch raw[i+1] {
		case 'b':
			out = append(out, '\b')
		case 'f':
			out = append(out, '\f')
		case 'n':
			out = append(out, '\n')
		case 'r':
			out = append(out, '\r')
		case 't':
			out = append(out, '\t')
		case 'u':
			dec, l := escapeSize(raw[i:])
			r, _ := hex4(raw[i+2:])
			switch {
			case l == 12:
				r2, _ := hex4(raw[i+8:])
				out = utf8.AppendRune(out, rune(0x10000+(r-0xD800)<<10+(r2-0xDC00)))
			case dec == 3 && r >= 0xD800 && r < 0xE000:
				out = utf8.AppendRune(out, utf8.RuneError)
			default:
				out = utf8.AppendRune(out, rune(r))
			}
			i += l
			continue
		default: // " \ /
			out = append(out, raw[i+1])
		}
		i += 2
	}
	return string(out)
}

func (p *parser) array(depth int) *Node {
	n := &Node{Kind: Array, Start: p.p}
	p.p++
	p.ws()
	if p.p < len(p.d) && p.d[p.p] == ']' {
		p.p++
		n.End = p.p
		return n
	}
	for {
		p.ws()
		n.Items = append(n.Items, p.value(depth+1))
		p.ws()
		if p.p >= len(p.d) {
			p.fail(n.Start, "unterminated array")
		}
		if p.d[p.p] == ',' {
			p.p++
			continue
		}
		if p.d[p.p] == ']' {
			p.p++
			n.End = p.p
			return n
		}
		p.fail(p.p, "expected ',' or ']'")
	}
}

func (p *parser) object(depth int) *Node {
	n := &Node{Kind: Object, Start: p.p}
	p.p++
	p.ws()
	if p.p < len(p.d) && p.d[p.p] == '}' {
		p.p++
		n.End = p.p
		return n
	}
	seen := map[string]struct{}{}
	for {
		p.ws()
		if p.p >= len(p.d) || p.d[p.p] != '"' {
			p.fail(p.p, "expected a string key")
		}
		k := p.str()
		p.ws()
		if p.p >= len(p.d) || p.d[p.p] != ':' {
			p.fail(p.p, "expected ':'")
		}
		p.p++
		p.ws()
		v := p.value(depth + 1)
		if _, dup := seen[k.Str]; dup {
			p.dups = append(p.dups, Dup{k.Str, k.Start, k.End})
		}
		seen[k.Str] = struct{}{}
		n.Members = append(n.Members, Member{k.Str, k.Start, k.End, v})
		p.ws()
		if p.p >= len(p.d) {
			p.fail(n.Start, "unterminated object")
		}
		if p.d[p.p] == ',' {
			p.p++
			continue
		}
		if p.d[p.p] == '}' {
			p.p++
			n.End = p.p
			return n
		}
		p.fail(p.p, "expected ',' or '}'")
	}
}

// Value converts a node to plain Go values for schema validation
// (objects as map[string]any with the first occurrence of a key winning).
func Value(n *Node) any {
	switch n.Kind {
	case Null:
		return nil
	case Bool:
		return n.Str == "true"
	case Number:
		return numberValue(n.Str)
	case String:
		return n.Str
	case Array:
		a := make([]any, len(n.Items))
		for i, it := range n.Items {
			a[i] = Value(it)
		}
		return a
	}
	m := make(map[string]any, len(n.Members))
	for _, mb := range n.Members {
		if _, ok := m[mb.Key]; !ok {
			m[mb.Key] = Value(mb.Value)
		}
	}
	return m
}

func numberValue(s string) any {
	if i, err := strconv.ParseInt(s, 10, 64); err == nil {
		return i
	}
	f, _ := strconv.ParseFloat(s, 64)
	return f
}
