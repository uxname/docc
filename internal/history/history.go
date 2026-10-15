// Package history reads the Git history of the documents and follows every
// block (a string of an array, a field value) through it, so the HTML file can
// show what a paragraph said in each commit.
package history

import (
	"bufio"
	"bytes"
	"fmt"
	"io"
	"os/exec"
	"path"
	"strconv"
	"strings"

	"github.com/uxname/docc/internal/engine"
	"github.com/uxname/docc/internal/loader"
)

// Commit is one commit that touched the documents. The working tree, when it
// differs from HEAD, is the last entry with an empty Hash.
type Commit struct {
	Hash    string `json:"h"`
	Author  string `json:"a"`
	Date    string `json:"d"`
	Subject string `json:"m"`
}

// Version is the text of a block after commit C (an index into Commits).
type Version struct {
	C    int    `json:"c"`
	Text string `json:"t"`
}

// Change is what one commit did to a document.
type Change struct {
	C       int        `json:"c"`
	Changed [][]string `json:"ch,omitempty"`  // [pointer, old text, new text]
	Added   [][]string `json:"add,omitempty"` // [pointer, text]
	Removed [][]string `json:"rm,omitempty"`  // [pointer, last text]
	Created bool       `json:"new,omitempty"`
}

// Doc is the history of one document: the versions of its current blocks
// (only those where the text changed) and its changes, oldest first.
type Doc struct {
	Blocks  map[string][]Version `json:"b"`
	Changes []Change             `json:"ch"`
}

// History of the documents; Docs is keyed by the path relative to src.
type History struct {
	Commits []Commit        `json:"commits"`
	Docs    map[string]*Doc `json:"docs"`
	Head    string          `json:"head,omitempty"`
}

// Load reads the history of the JSON files in srcDir. current maps the path
// of every document relative to srcDir to its parsed root as it is on disk;
// a document that differs from HEAD gets a last version from the working
// tree. Without git or a repository it returns nil.
// ponytail: commits are replayed in log order with the full file at each one;
// on merged branches the order is by date, not by topology.
func Load(srcDir string, current map[string]*loader.Node) *History {
	prefix, err := git(srcDir, nil, "rev-parse", "--show-prefix")
	if err != nil {
		return nil
	}
	prefix = strings.TrimSpace(prefix)
	h := &History{Docs: map[string]*Doc{}}
	if head, err := git(srcDir, nil, "rev-parse", "--short=8", "HEAD"); err == nil {
		h.Head = strings.TrimSpace(head)
	}
	log, err := git(srcDir, nil, "-c", "core.quotePath=false", "log", "--reverse", "-M", "--name-status",
		"--format=%x01%H%x1f%an%x1f%aI%x1f%s", "--", ".")
	if err != nil {
		return h
	}
	type entry struct {
		c         int
		status    string
		old, path string // relative to srcDir
	}
	var entries []entry
	var full []string // full hashes by commit index
	for _, rec := range strings.Split(log, "\x01")[1:] {
		lines := strings.Split(rec, "\n")
		f := strings.Split(lines[0], "\x1f")
		if len(f) < 4 {
			continue
		}
		c := len(h.Commits)
		h.Commits = append(h.Commits, Commit{Hash: f[0][:8], Author: f[1], Date: f[2], Subject: f[3]})
		full = append(full, f[0])
		for _, l := range lines[1:] {
			p := strings.Split(l, "\t")
			if len(p) < 2 {
				continue
			}
			e := entry{c: c, status: p[0][:1], path: p[len(p)-1]}
			if e.status == "R" || e.status == "C" {
				e.old = p[1]
			}
			rel := func(s string) (string, bool) {
				r, ok := strings.CutPrefix(s, prefix)
				return r, ok && path.Ext(r) == ".json"
			}
			var ok bool
			if e.path, ok = rel(e.path); !ok {
				continue
			}
			if e.old != "" {
				e.old, _ = rel(e.old)
			}
			entries = append(entries, e)
		}
	}
	// read every blob the replay needs through one cat-file process
	var req bytes.Buffer
	for _, e := range entries {
		if e.status != "D" {
			fmt.Fprintf(&req, "%s:%s\n", full[e.c], prefix+e.path)
		}
	}
	blobs, err := catFiles(srcDir, &req)
	if err != nil {
		return h
	}
	state := map[string]*replay{}
	for _, e := range entries {
		switch e.status {
		case "D":
			delete(state, e.path)
			continue
		case "R":
			if st := state[e.old]; st != nil {
				state[e.path] = st
				delete(state, e.old)
			}
		}
		data := blobs[0]
		blobs = blobs[1:]
		root := parse(data)
		if root == nil { // a commit with broken JSON: keep the last good version
			continue
		}
		if state[e.path] == nil {
			state[e.path] = &replay{doc: &Doc{}}
		}
		state[e.path].step(root, e.c)
	}
	work := -1
	for rel, root := range current {
		st := state[rel]
		if st == nil {
			st = &replay{doc: &Doc{}}
			state[rel] = st
		}
		if st.root != nil && same(st.root, root) {
			continue
		}
		if work < 0 {
			work = len(h.Commits)
			h.Commits = append(h.Commits, Commit{})
		}
		st.step(root, work)
	}
	for rel := range current {
		st := state[rel]
		st.doc.Blocks = map[string][]Version{}
		for p, vs := range st.blocks {
			st.doc.Blocks[p] = vs
		}
		h.Docs[rel] = st.doc
	}
	return h
}

func parse(data []byte) *loader.Node {
	res := loader.Parse(data)
	if res.Err != nil || res.Root == nil || res.Root.Kind != loader.Object {
		return nil
	}
	return res.Root
}

func same(a, b *loader.Node) bool {
	fa, fb := Flatten(a), Flatten(b)
	if len(fa) != len(fb) {
		return false
	}
	for i := range fa {
		if fa[i] != fb[i] {
			return false
		}
	}
	return true
}

// replay is the state of one file while the commits are replayed.
type replay struct {
	root   *loader.Node
	leaves []Leaf
	blocks map[string][]Version
	doc    *Doc
}

func (st *replay) step(root *loader.Node, c int) {
	leaves := Flatten(root)
	match := Align(st.leaves, leaves)
	old := map[string]Leaf{}
	for _, l := range st.leaves {
		old[l.Ptr] = l
	}
	ch := Change{C: c, Created: st.root == nil}
	blocks := map[string][]Version{}
	kept := map[string]bool{}
	for _, l := range leaves {
		prev, ok := match[l.Ptr]
		vs := []Version(nil)
		if ok {
			kept[prev] = true
			vs = append(vs, st.blocks[prev]...)
			if old[prev].Text != l.Text {
				vs = append(vs, Version{c, l.Text})
				ch.Changed = append(ch.Changed, []string{l.Ptr, old[prev].Text, l.Text})
			}
		} else {
			vs = []Version{{c, l.Text}}
			if !ch.Created {
				ch.Added = append(ch.Added, []string{l.Ptr, l.Text})
			}
		}
		blocks[l.Ptr] = vs
	}
	for _, l := range st.leaves {
		if !kept[l.Ptr] {
			ch.Removed = append(ch.Removed, []string{l.Ptr, l.Text})
		}
	}
	if ch.Created || len(ch.Changed)+len(ch.Added)+len(ch.Removed) > 0 {
		st.doc.Changes = append(st.doc.Changes, ch)
	}
	st.root, st.leaves, st.blocks = root, leaves, blocks
}

// Leaf is one block of a document: Parent is the pointer of the array of
// strings it belongs to ("" for a block that is not an element of one).
type Leaf struct {
	Ptr, Parent, Text string
}

// Flatten lists the blocks of a document in file order: every string of an
// array of strings, every other field or section value as a whole. Sections
// are entered. The pointers are those of Pointers.
func Flatten(root *loader.Node) []Leaf {
	var out []Leaf
	var walk func(n *loader.Node, ptr string, top bool)
	walk = func(n *loader.Node, ptr string, top bool) {
		switch {
		case n.Kind == loader.Object && (top || engine.IsSections(n)):
			for _, m := range n.Members {
				if m.Key != engine.SectionsKey {
					walk(m.Value, ptr+"/"+Escape(m.Key), false)
				}
			}
		case n.Kind == loader.Array && allStrings(n):
			for i, it := range n.Items {
				out = append(out, Leaf{ptr + "/" + strconv.Itoa(i), ptr, it.Str})
			}
		default:
			out = append(out, Leaf{Ptr: ptr, Text: Text(n)})
		}
	}
	walk(root, "", true)
	return out
}

func allStrings(n *loader.Node) bool {
	for _, it := range n.Items {
		if it.Kind != loader.String {
			return false
		}
	}
	return len(n.Items) > 0
}

// Text is the plain text of a block: a string as it is, a list or table one
// item per line, an object as "key: value" pairs.
func Text(n *loader.Node) string {
	switch n.Kind {
	case loader.Array:
		var parts []string
		for _, it := range n.Items {
			parts = append(parts, Text(it))
		}
		return strings.Join(parts, "\n")
	case loader.Object:
		var parts []string
		for _, m := range n.Members {
			parts = append(parts, m.Key+": "+Text(m.Value))
		}
		return strings.Join(parts, "; ")
	case loader.Null:
		return "null"
	}
	return n.Str
}

// Escape escapes a key for a JSON Pointer (RFC 6901).
func Escape(k string) string {
	return strings.ReplaceAll(strings.ReplaceAll(k, "~", "~0"), "/", "~1")
}

// Pointers maps every node of a document to its JSON Pointer.
func Pointers(root *loader.Node) map[*loader.Node]string {
	out := map[*loader.Node]string{}
	var walk func(n *loader.Node, ptr string)
	walk = func(n *loader.Node, ptr string) {
		out[n] = ptr
		for i, it := range n.Items {
			walk(it, ptr+"/"+strconv.Itoa(i))
		}
		for _, m := range n.Members {
			walk(m.Value, ptr+"/"+Escape(m.Key))
		}
	}
	walk(root, "")
	return out
}

// pair matches the unmatched strings of one gap: the most similar pairs
// first (Dice coefficient of character bigrams at least 0.3); what is left
// pairs in order when both sides have the same count, as a rewrite.
// ponytail: greedy over all pairs, O(n²) per gap; gaps are a few paragraphs.
func pair(a, b []Leaf, match map[string]string) {
	usedA, usedB := make([]bool, len(a)), make([]bool, len(b))
	ba, bb := make([]map[string]int, len(a)), make([]map[string]int, len(b))
	for i := range a {
		ba[i] = bigrams(a[i].Text)
	}
	for j := range b {
		bb[j] = bigrams(b[j].Text)
	}
	for {
		best, bi, bj := 0.3, -1, -1
		for i := range a {
			for j := range b {
				if !usedA[i] && !usedB[j] {
					if s := dice(ba[i], bb[j]); s >= best {
						best, bi, bj = s, i, j
					}
				}
			}
		}
		if bi < 0 {
			break
		}
		usedA[bi], usedB[bj] = true, true
		match[b[bj].Ptr] = a[bi].Ptr
	}
	var ra, rb []Leaf
	for i := range a {
		if !usedA[i] {
			ra = append(ra, a[i])
		}
	}
	for j := range b {
		if !usedB[j] {
			rb = append(rb, b[j])
		}
	}
	if len(ra) == len(rb) {
		for k := range ra {
			match[rb[k].Ptr] = ra[k].Ptr
		}
	}
}

// Similar is the Dice coefficient of the character bigrams of two texts: 1
// for equal texts, 0 for texts without a common pair of letters.
func Similar(a, b string) float64 { return dice(bigrams(a), bigrams(b)) }

func bigrams(s string) map[string]int {
	r := []rune(strings.ToLower(s))
	m := map[string]int{}
	for i := 0; i+1 < len(r); i++ {
		m[string(r[i:i+2])]++
	}
	if len(r) == 1 {
		m[string(r)]++
	}
	return m
}

func dice(a, b map[string]int) float64 {
	na, nb, common := 0, 0, 0
	for k, v := range a {
		na += v
		common += min(v, b[k])
	}
	for _, v := range b {
		nb += v
	}
	if na+nb == 0 {
		return 0
	}
	return 2 * float64(common) / float64(na+nb)
}

// Align maps the pointers of the new blocks to the blocks they continue.
// Blocks outside arrays match by pointer. The strings of one array match by
// a longest common subsequence of equal texts; unmatched strings between two
// matches pair up by similarity (see pair), the rest are added or removed.
func Align(old, cur []Leaf) map[string]string {
	match := map[string]string{}
	oldPtr := map[string]bool{}
	oldArr := map[string][]Leaf{}
	for _, l := range old {
		if l.Parent == "" {
			oldPtr[l.Ptr] = true
		} else {
			oldArr[l.Parent] = append(oldArr[l.Parent], l)
		}
	}
	curArr := map[string][]Leaf{}
	var order []string
	for _, l := range cur {
		if l.Parent == "" {
			if oldPtr[l.Ptr] {
				match[l.Ptr] = l.Ptr
			}
			continue
		}
		if curArr[l.Parent] == nil {
			order = append(order, l.Parent)
		}
		curArr[l.Parent] = append(curArr[l.Parent], l)
	}
	for _, p := range order {
		a, b := oldArr[p], curArr[p]
		// LCS table over equal texts
		n, m := len(a), len(b)
		dp := make([][]int, n+1)
		for i := range dp {
			dp[i] = make([]int, m+1)
		}
		for i := n - 1; i >= 0; i-- {
			for j := m - 1; j >= 0; j-- {
				if a[i].Text == b[j].Text {
					dp[i][j] = dp[i+1][j+1] + 1
				} else {
					dp[i][j] = max(dp[i+1][j], dp[i][j+1])
				}
			}
		}
		var ga, gb []Leaf // the current gap of unmatched strings
		flush := func() {
			pair(ga, gb, match)
			ga, gb = nil, nil
		}
		i, j := 0, 0
		for i < n && j < m {
			switch {
			case a[i].Text == b[j].Text:
				flush()
				match[b[j].Ptr] = a[i].Ptr
				i, j = i+1, j+1
			case dp[i+1][j] >= dp[i][j+1]:
				ga = append(ga, a[i])
				i++
			default:
				gb = append(gb, b[j])
				j++
			}
		}
		ga, gb = append(ga, a[i:]...), append(gb, b[j:]...)
		flush()
	}
	return match
}

func git(dir string, stdin io.Reader, args ...string) (string, error) {
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	cmd.Stdin = stdin
	out, err := cmd.Output()
	return string(out), err
}

// catFiles returns the contents of the objects named one per line in req, in order.
func catFiles(dir string, req io.Reader) ([][]byte, error) {
	cmd := exec.Command("git", "cat-file", "--batch")
	cmd.Dir = dir
	cmd.Stdin = req
	out, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	var blobs [][]byte
	r := bufio.NewReader(out)
	for {
		line, err := r.ReadString('\n')
		if err != nil {
			break
		}
		f := strings.Fields(line)
		if len(f) < 3 { // "<name> missing"
			blobs = append(blobs, nil)
			continue
		}
		size, _ := strconv.Atoi(f[2])
		b := make([]byte, size+1) // content and the closing newline
		if _, err := io.ReadFull(r, b); err != nil {
			break
		}
		blobs = append(blobs, b[:size])
	}
	return blobs, cmd.Wait()
}
