package engine

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io/fs"
	"net/url"
	"os"
	"sort"
	"strings"
	"unicode"
)

// LockFile is the link lock next to docc.json.
const LockFile = "docc.lock"

// Link is one (source, target) pair of the lock: a document id or a code file,
// and a target as written. Hash is the hash of the target value, Code the hash
// of the code regions of a region link ("" otherwise).
type Link struct {
	Source, Target string
	Hash, Code     string

	doc        *Doc // source document, nil for a code file
	start, end int  // reference in doc
	line, col  int  // tag in the code file
	at         int  // line in docc.lock
}

func (l Link) key() string { return l.Source + " " + l.Target }

func (l Link) String() string {
	s := lockEsc(l.Source) + " " + lockEsc(l.Target) + " " + l.Hash
	if l.Code != "" {
		s += " " + l.Code
	}
	return s
}

// lockEsc escapes '%' and white space as %XX so a line splits on spaces;
// a section key such as "2.1 Overview" stays readable otherwise.
func lockEsc(s string) string {
	var b strings.Builder
	for _, r := range s {
		if r == '%' || unicode.IsSpace(r) || unicode.IsControl(r) {
			for _, c := range []byte(string(r)) {
				fmt.Fprintf(&b, "%%%02X", c)
			}
			continue
		}
		b.WriteRune(r)
	}
	return b.String()
}

func hash8(s string) string {
	sum := sha256.Sum256([]byte(s))
	return hex.EncodeToString(sum[:])[:8]
}

func isHash(s string) bool {
	_, err := hex.DecodeString(s)
	return len(s) == 8 && err == nil && strings.ToLower(s) == s
}

// lockSkip are the top-level keys a whole-document target does not depend on.
var lockSkip = map[string]bool{"notes": true, "docIgnore": true, "hidden": true, "order": true, "parent": true}

// targetHash hashes the canonical JSON of the value a target points to: the
// document without service fields, order and parent; a field or section; the
// text of an anchored paragraph. ok is false when the target does not resolve.
func (p *Project) targetHash(target string) (h string, ok bool) {
	id, field, hasField, ok := splitRef(target)
	t := p.Graph[id]
	if !ok || t == nil || !t.Parsed() {
		return "", false
	}
	var b strings.Builder
	switch {
	case !hasField:
		canonical(&b, t.Root, lockSkip)
	default:
		if msg, _ := fragmentProblem(t, id, field); msg != "" {
			return "", false
		}
		if name, block := strings.CutPrefix(field, "^"); block {
			text, _ := t.Block(t.Blocks[name].Node)
			quote(&b, text)
		} else {
			canonical(&b, Resolve(t.Root, field), nil)
		}
	}
	return hash8(b.String()), true
}

// Links returns the current links sorted by key, with the hashes of their
// targets now: [[...]] in the text and notes of every document (hidden too)
// and docc: tags in code. Self links and targets that do not resolve are left
// out; a pair seen twice keeps its first place.
func (p *Project) Links() []Link {
	seen := map[string]bool{}
	var out []Link
	add := func(l Link) {
		h, ok := p.targetHash(l.Target)
		if !ok || seen[l.key()] {
			return
		}
		seen[l.key()] = true
		l.Hash = h
		out = append(out, l)
	}
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) {
			continue
		}
		for _, r := range d.Refs {
			if r.OK && r.Target != d.ID {
				add(Link{Source: d.ID, Target: r.Inner, doc: d, start: r.Start, end: r.End})
			}
		}
	}
	keys := make([]Link, 0, len(p.Code))
	for k, h := range p.Code {
		at := p.codeAt[k]
		if h != "" {
			h = h[:8]
		}
		keys = append(keys, Link{Source: k.File, Target: k.Target, Code: h, line: at[0], col: at[1]})
	}
	sort.Slice(keys, func(i, j int) bool { return keys[i].key() < keys[j].key() })
	for _, l := range keys {
		add(l)
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].key() < out[j].key() })
	return out
}

// readLock returns the lines of docc.lock that parse and the number of the
// first one that does not (0: none).
func (p *Project) readLock() (data string, ls []Link, bad int, exists bool) {
	b, err := os.ReadFile(p.Cfg.Abs(LockFile))
	if err != nil {
		return "", nil, 0, false
	}
	data = strings.ReplaceAll(string(b), "\r\n", "\n") // a checkout with autocrlf
	for i, row := range lockRows(data) {
		if l, ok := parseLockLine(row); ok {
			l.at = i + 1
			ls = append(ls, l)
		} else if bad == 0 {
			bad = i + 1
		}
	}
	return data, ls, bad, true
}

func lockRows(data string) []string {
	if data == "" {
		return nil
	}
	return strings.Split(strings.TrimSuffix(data, "\n"), "\n")
}

func parseLockLine(row string) (l Link, ok bool) {
	f := strings.Split(row, " ")
	if len(f) != 3 && len(f) != 4 {
		return l, false
	}
	var e1, e2 error
	l.Source, e1 = url.PathUnescape(f[0])
	l.Target, e2 = url.PathUnescape(f[1])
	l.Hash = f[2]
	if len(f) == 4 {
		l.Code = f[3]
		ok = isHash(l.Code)
	} else {
		ok = true
	}
	return l, ok && e1 == nil && e2 == nil && l.Source != "" && l.Target != "" && isHash(l.Hash)
}

// firstByKey maps a key to its first line in the lock.
func firstByKey(ls []Link) map[string]Link {
	m := map[string]Link{}
	for _, l := range ls {
		if _, ok := m[l.key()]; !ok {
			m[l.key()] = l
		}
	}
	return m
}

// lockText is what docc build writes: every current link, with the hashes
// stored for it when the lock has it, else with the current ones.
func lockText(cur, stored []Link) string {
	old := firstByKey(stored)
	var b strings.Builder
	for _, l := range cur {
		if o, ok := old[l.key()]; ok {
			l.Hash = o.Hash
			if o.Code != "" && l.Code != "" {
				l.Code = o.Code
			}
		}
		b.WriteString(l.String() + "\n")
	}
	return b.String()
}

// WriteLock rewrites docc.lock for docc build: new links are confirmed with
// their current hashes, vanished ones dropped. Without links there is no file.
func (p *Project) WriteLock() error {
	data, stored, _, exists := p.readLock()
	want := lockText(p.Links(), stored)
	switch {
	case want == data && exists:
		return nil
	case want == "":
		if err := os.Remove(p.Cfg.Abs(LockFile)); err != nil && !errors.Is(err, fs.ErrNotExist) {
			return err
		}
		return nil
	}
	return os.WriteFile(p.Cfg.Abs(LockFile), []byte(want), 0o644)
}

// checkLock reports links whose target (DOC209) or code region (DOC402)
// changed after they were confirmed in docc.lock.
func (p *Project) checkLock() {
	_, stored, _, _ := p.readLock()
	if len(stored) == 0 {
		return
	}
	old := firstByKey(stored)
	for _, l := range p.Links() {
		o, ok := old[l.key()]
		if !ok {
			continue
		}
		if o.Hash != l.Hash {
			msg := fmt.Sprintf("the target %s changed after the link from %s was confirmed", l.Target, l.Source)
			if l.doc != nil {
				p.emit(l.doc, "DOC209", l.start, l.end, msg, fmt.Sprintf("Check %s against %s, then run docc ack %s.", l.Source, l.Target, l.Target))
			} else {
				p.emitFile(l.Source, "DOC209", l.line, l.col, msg, fmt.Sprintf("Check the code in %s against %s, then run docc ack %s.", l.Source, l.Target, l.Target))
			}
		}
		if o.Code != "" && l.Code != "" && o.Code != l.Code {
			p.emitFile(l.Source, "DOC402", l.line, l.col, fmt.Sprintf("code region in %s for %s changed", l.Source, l.Target),
				fmt.Sprintf("Check the document %s still matches, then run docc ack %s.", l.Target, l.Source))
		}
	}
}

// CheckLock reports DOC503 when docc.lock is not what docc build would write:
// a link is missing or extra, a key repeats, a line is malformed.
func (p *Project) CheckLock() {
	data, stored, bad, exists := p.readLock()
	cur := p.Links()
	if lockText(cur, stored) == data {
		return
	}
	line, msg := 1, "docc.lock is not in the form docc build writes"
	now := map[string]bool{}
	for _, l := range cur {
		now[l.key()] = true
	}
	first := firstByKey(stored)
	switch {
	case !exists:
		msg = fmt.Sprintf("docc.lock is missing, and the sources have %d links", len(cur))
	case bad > 0:
		line, msg = bad, fmt.Sprintf("line %d is not \"<source> <target> <hash> [<code hash>]\"", bad)
	default:
		found := false
		for _, l := range stored {
			if first[l.key()].at != l.at {
				line, msg, found = l.at, fmt.Sprintf("the link from %s to %s is listed twice", l.Source, l.Target), true
			} else if !now[l.key()] {
				line, msg, found = l.at, fmt.Sprintf("the link from %s to %s no longer exists", l.Source, l.Target), true
			}
			if found {
				break
			}
		}
		for _, l := range cur {
			if _, ok := first[l.key()]; !ok && !found {
				msg, found = fmt.Sprintf("the link from %s to %s is not in docc.lock", l.Source, l.Target), true
			}
		}
	}
	p.emitFile(LockFile, "DOC503", line, 1, msg, "Run docc build and commit docc.lock.")
}

// Ack confirms the lock lines around each name: its source or target is the
// name, or the target lies in the document with that id; with all, every
// line. Lines of links that no longer exist are left to docc build.
func (p *Project) Ack(names []string, all bool) (int, error) {
	data, _, _, exists := p.readLock()
	if !exists {
		return 0, &UsageError{"docc.lock not found: run docc build first"}
	}
	cur := map[string]Link{}
	for _, l := range p.Links() {
		cur[l.key()] = l
	}
	rows := lockRows(data)
	hit := map[string]bool{}
	n := 0
	for i, row := range rows {
		l, ok := parseLockLine(row)
		c, now := cur[l.key()]
		if !ok || !now {
			continue
		}
		match := all
		id, _, _, _ := splitRef(l.Target)
		for _, x := range names {
			if l.Source == x || l.Target == x || id == x {
				match, hit[x] = true, true
			}
		}
		if match {
			l.Hash, l.Code = c.Hash, c.Code
			rows[i] = l.String()
			n++
		}
	}
	for _, x := range names {
		if !hit[x] {
			return 0, &UsageError{fmt.Sprintf("no link in docc.lock matches %q (a new link gets there with docc build)", x)}
		}
	}
	if n == 0 {
		return 0, nil
	}
	return n, os.WriteFile(p.Cfg.Abs(LockFile), []byte(strings.Join(rows, "\n")+"\n"), 0o644)
}
