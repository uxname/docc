package engine

import (
	"fmt"
	"sort"

	"github.com/santiment/docc/internal/loader"
)

// inGraph reports whether d is the graph node of its id (the first document
// by path) and parses.
func (p *Project) inGraph(d *Doc) bool { return p.Graph[d.ID] == d }

// parentOf returns the parent value node when it is a string.
func parentOf(d *Doc) *loader.Node {
	if d.Root == nil {
		return nil
	}
	if n := d.Root.Get("parent"); n != nil && n.Kind == loader.String {
		return n
	}
	return nil
}

// checkGraph emits DOC201-DOC205, DOC210 and DOC211.
func (p *Project) checkGraph() {
	p.dupIDs()
	p.brokenRefs()
	p.parentCycles()
	p.roots()
	p.includes()
}

func (p *Project) dupIDs() {
	for _, d := range p.Docs {
		if first := p.Graph[d.ID]; first != d {
			p.emitNoPos(d, "DOC201", fmt.Sprintf("id %q is already used by %s", d.ID, first.File),
				"Rename one of the files: the file name (without .json) is the document id and must be unique.")
		}
	}
}

// target describes a reference target for DOC202 and DOC210.
func (p *Project) refTarget(id string) (t *Doc, exists bool) {
	t = p.Graph[id]
	return t, t != nil
}

func (p *Project) brokenRefs() {
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) {
			continue
		}
		check := func(id string, start, end int, what string, visibleOnly bool) {
			t, ok := p.refTarget(id)
			if !ok {
				p.emit(d, "DOC202", start, end, fmt.Sprintf("%s points to a document that does not exist: %q", what, id),
					fmt.Sprintf("Create %s/%s.json or fix the id.", p.Cfg.Src, id))
				return
			}
			if visibleOnly && !d.Hidden && t.Parsed() && t.Hidden {
				p.emit(d, "DOC210", start, end, fmt.Sprintf("%s points to the hidden document %q", what, id),
					"A visible document cannot link to a hidden one: make the target visible, or move the reference into notes.")
			}
		}
		if n := parentOf(d); n != nil {
			check(n.Str, n.Start, n.End, "parent", true)
		}
		for _, n := range StringList(d.Root.Get("links")) {
			check(n.Str, n.Start, n.End, "links", true)
		}
		for _, r := range d.Refs {
			if !r.OK {
				p.emit(d, "DOC202", r.Start, r.End, fmt.Sprintf("malformed reference %q", "[["+r.Inner+"]]"),
					"Write [[id]] or [[id#field]]: the id uses only a-z, 0-9 and '-'.")
				continue
			}
			check(r.Target, r.Start, r.End, "reference [["+r.Inner+"]]", !r.Notes)
		}
	}
}

func (p *Project) parentCycles() {
	next := map[string]string{}
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) {
			continue
		}
		if n := parentOf(d); n != nil {
			if _, ok := p.Graph[n.Str]; ok {
				next[d.ID] = n.Str
			}
		}
	}
	// a node is on a cycle if following parents returns to it
	state := map[string]int{} // 1 visiting, 2 done
	onCycle := map[string]bool{}
	for _, d := range p.Docs {
		id := d.ID
		if _, ok := next[id]; !ok || state[id] != 0 {
			continue
		}
		var chain []string
		cur := id
		for {
			if state[cur] == 1 {
				// cycle: from the first occurrence of cur in chain
				i := len(chain) - 1
				for chain[i] != cur {
					i--
				}
				for _, c := range chain[i:] {
					onCycle[c] = true
				}
				break
			}
			if state[cur] == 2 {
				break
			}
			state[cur] = 1
			chain = append(chain, cur)
			nx, ok := next[cur]
			if !ok {
				break
			}
			cur = nx
		}
		for _, c := range chain {
			state[c] = 2
		}
	}
	for _, d := range p.Docs {
		if onCycle[d.ID] && p.inGraph(d) {
			n := parentOf(d)
			if n.Str == d.ID {
				continue // a self-parent is reported once, as DOC206
			}
			p.emit(d, "DOC204", n.Start, n.End, fmt.Sprintf("the parent chain of %q forms a cycle", d.ID),
				"Break the cycle: point one document in it to a different parent.")
		}
	}
}

// roots checks DOC205 and DOC203.
func (p *Project) roots() {
	var starts []*Doc
	for _, id := range p.Cfg.Roots {
		t := p.Graph[id]
		if t == nil || (t.Parsed() && t.Hidden) {
			what := "does not exist"
			if t != nil {
				what = "is hidden"
			}
			line, col := p.Cfg.RootsPos[0], p.Cfg.RootsPos[1]
			if line == 0 {
				line, col = 1, 1
			}
			hint := fmt.Sprintf("Create %s/%s.json, or change roots in %s.", p.Cfg.Src, id, p.Cfg.File)
			if p.Cfg.RootsPos[0] == 0 {
				p.emitFile(p.Cfg.File, "DOC205", 1, 1, fmt.Sprintf("root %q %s", id, what), hint)
			} else {
				p.emitFile(p.Cfg.File, "DOC205", line, col, fmt.Sprintf("root %q %s", id, what), hint)
			}
			continue
		}
		starts = append(starts, t)
	}
	if len(starts) == 0 {
		return // reachability is undefined without a root
	}
	adj := map[string][]string{}
	link := func(a, b string) {
		adj[a] = append(adj[a], b)
		adj[b] = append(adj[b], a)
	}
	visible := func(id string) bool {
		t := p.Graph[id]
		return t != nil && (!t.Parsed() || !t.Hidden) // a file with DOC101 stays a node
	}
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) || d.Hidden {
			continue
		}
		if n := parentOf(d); n != nil && visible(n.Str) {
			link(d.ID, n.Str)
		}
		for _, n := range StringList(d.Root.Get("links")) {
			if visible(n.Str) {
				link(d.ID, n.Str)
			}
		}
		for _, r := range d.Refs {
			if r.OK && !r.Notes && visible(r.Target) {
				link(d.ID, r.Target)
			}
		}
	}
	seen := map[string]bool{}
	queue := []string{}
	for _, s := range starts {
		seen[s.ID] = true
		queue = append(queue, s.ID)
	}
	for len(queue) > 0 {
		id := queue[0]
		queue = queue[1:]
		for _, nb := range adj[id] {
			if !seen[nb] {
				seen[nb] = true
				queue = append(queue, nb)
			}
		}
	}
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) || d.Hidden || seen[d.ID] {
			continue
		}
		// a broken parent is already reported (DOC105 / DOC206): no cascade
		if pn := d.Root.Get("parent"); pn != nil && (pn.Kind != loader.String || pn.Str == d.ID) {
			continue
		}
		p.emitNoPos(d, "DOC203", fmt.Sprintf("document %q is not reachable from the roots %v", d.ID, p.Cfg.Roots),
			"Link it from a reachable document (links or [[id]] in the text) or set its parent.")
	}
}

type incNode struct{ doc, field string }

// includes validates {{id#field}} (DOC211) and visibility (DOC210).
func (p *Project) includes() {
	edges := map[incNode][]incNode{}
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) {
			continue
		}
		for i := range d.Incs {
			u := &d.Incs[i]
			bad := func(msg, hint string) {
				p.emit(d, "DOC211", u.Start, u.End, msg, hint)
			}
			if !u.OK {
				bad(fmt.Sprintf("malformed include %q", "{{"+u.Inner+"}}"),
					"Write {{id#field}}: the id of an existing document, '#', and a top-level field or field/section with text.")
				continue
			}
			t := p.Graph[u.Target]
			if t == nil {
				bad(fmt.Sprintf("include points to a document that does not exist: %q", u.Target),
					fmt.Sprintf("Create %s/%s.json or fix the id.", p.Cfg.Src, u.Target))
				continue
			}
			if !t.Parsed() {
				continue // DOC101 on the target already
			}
			if ServiceFields[u.Field] {
				bad(fmt.Sprintf("include cannot use the service field %q", u.Field), "Include a content field such as body or a free field.")
				continue
			}
			v := Resolve(t.Root, u.Field)
			if v == nil {
				bad(fmt.Sprintf("document %q has no field %q", u.Target, u.Field), "Use an existing top-level field or field/section.")
				continue
			}
			if !isText(v) {
				bad(fmt.Sprintf("field %q of %q is not text", u.Field, u.Target), "Include a field that is a string or a non-empty array of strings.")
				continue
			}
			u.Valid = true
			if !d.Hidden && t.Hidden {
				p.emit(d, "DOC210", u.Start, u.End, fmt.Sprintf("include points to the hidden document %q", u.Target),
					"A visible document cannot include text from a hidden one: make the target visible or remove the include.")
			}
			from := incNode{d.ID, u.SrcField}
			edges[from] = append(edges[from], incNode{u.Target, u.Field})
		}
	}
	cyclic := cyclicNodes(edges)
	if len(cyclic) == 0 {
		return
	}
	// an include is bad when its target reaches a cycle
	reach := reachCyclic(edges, cyclic)
	for _, d := range p.Docs {
		if !d.Parsed() || !p.inGraph(d) {
			continue
		}
		for i := range d.Incs {
			u := &d.Incs[i]
			if u.Valid && reach[incNode{u.Target, u.Field}] {
				u.Valid = false
				p.emit(d, "DOC211", u.Start, u.End, fmt.Sprintf("include {{%s}} leads into an include cycle", u.Inner),
					"Break the cycle: a field cannot include itself, directly or through other includes.")
			}
		}
	}
}

func isText(n *loader.Node) bool {
	switch n.Kind {
	case loader.String:
		return true
	case loader.Array:
		if len(n.Items) == 0 {
			return false
		}
		for _, it := range n.Items {
			if it.Kind != loader.String {
				return false
			}
		}
		return true
	}
	return false
}

// cyclicNodes returns nodes that can reach themselves.
func cyclicNodes(edges map[incNode][]incNode) map[incNode]bool {
	out := map[incNode]bool{}
	keys := make([]incNode, 0, len(edges))
	for k := range edges {
		keys = append(keys, k)
	}
	sort.Slice(keys, func(i, j int) bool {
		if keys[i].doc != keys[j].doc {
			return keys[i].doc < keys[j].doc
		}
		return keys[i].field < keys[j].field
	})
	for _, k := range keys {
		seen := map[incNode]bool{}
		stack := append([]incNode{}, edges[k]...)
		for len(stack) > 0 {
			n := stack[len(stack)-1]
			stack = stack[:len(stack)-1]
			if n == k {
				out[k] = true
				break
			}
			if seen[n] {
				continue
			}
			seen[n] = true
			stack = append(stack, edges[n]...)
		}
	}
	return out
}

// reachCyclic marks every node that is cyclic or reaches a cyclic node.
func reachCyclic(edges map[incNode][]incNode, cyclic map[incNode]bool) map[incNode]bool {
	rev := map[incNode][]incNode{}
	for a, bs := range edges {
		for _, b := range bs {
			rev[b] = append(rev[b], a)
		}
	}
	out := map[incNode]bool{}
	var stack []incNode
	for n := range cyclic {
		out[n] = true
		stack = append(stack, n)
	}
	for len(stack) > 0 {
		n := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		for _, pr := range rev[n] {
			if !out[pr] {
				out[pr] = true
				stack = append(stack, pr)
			}
		}
	}
	return out
}
