package main

import (
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/uxname/docc/internal/comments"
	"github.com/uxname/docc/internal/engine"
	"github.com/uxname/docc/internal/loader"
)

func init() {
	cmdUsage["comments"] = `Usage: docc comments [--all] | import [--internal] <file|->... | done [--reply TEXT] <id>... | rm <id>...

Review comments left on blocks of the HTML file (docc build --html) leave the page as text
copied for a messenger or as a downloaded JSON file. import adds them to docc.comments.json
in the project root, which belongs in git, as not processed; - reads the copied text from
standard input. A comment already there (same id) is skipped, and a comment whose block
moved is attached to the block with the quoted text. --internal marks the imported comments
as the team's own: they are not put into the HTML file.
Without a subcommand docc comments lists the comments not processed yet, each with the
quoted block and its current text; --all lists every comment. done marks comments as
processed, with --reply the answer the reader sees under the comment in the next HTML
file; rm deletes them. An id may be shortened to any unique prefix.

Examples:
  docc comments import ~/Downloads/comments-2026-10-15.json
  wl-paste | docc comments import -
  docc comments
  docc comments --format json
  docc comments done 3f2a91bc --reply "Added to section 5.2"
  docc comments rm 3f2a91bc
`
}

func cmdComments(c *common, args []string, stdout io.Writer) (int, error) {
	fs := newFlags("comments", c)
	all := fs.Bool("all", false, "list processed comments too")
	internal := fs.Bool("internal", false, "import: the team's own comments, kept out of the HTML file")
	reply := fs.String("reply", "", "done: the answer shown under the comment in the HTML file")
	pos, err := parse(fs, args)
	if err == nil {
		err = c.check()
	}
	if err != nil {
		return 2, err
	}
	sub := ""
	if len(pos) > 0 {
		sub, pos = pos[0], pos[1:]
	}
	switch sub {
	case "", "import", "done", "rm":
	default:
		return 2, usageErr("unknown subcommand %q: use import, done or rm", sub)
	}
	if sub == "" && len(pos) > 0 {
		return 2, usageErr("unexpected argument %q", pos[0])
	}
	if *internal && sub != "import" {
		return 2, usageErr("--internal is only for import")
	}
	if *reply != "" && sub != "done" {
		return 2, usageErr("--reply is only for done")
	}
	if sub != "" && len(pos) == 0 {
		if sub == "import" {
			return 2, usageErr("give the files to import")
		}
		return 2, usageErr("give the ids of the comments")
	}
	cfg, p, err := loadProject(c)
	if err != nil {
		return 2, err
	}
	items, err := comments.Load(cfg.Root)
	if err != nil {
		return 2, err
	}
	switch sub {
	case "import":
		return importComments(c, p, cfg.Root, items, pos, *internal, stdout)
	case "done", "rm":
		return changeComments(c, cfg.Root, items, pos, sub, *reply, stdout)
	}
	return listComments(c, p, items, *all, stdout)
}

// rootOf is the parsed document of a comment, nil when it is gone.
func rootOf(p *engine.Project, it comments.Item) *loader.Node {
	if d := p.Graph[it.Doc]; d != nil {
		return d.Root
	}
	return nil
}

// stdin is where import - reads the copied comments; tests replace it.
var stdin io.Reader = os.Stdin

func importComments(c *common, p *engine.Project, root string, items []comments.Item, files []string, internal bool, stdout io.Writer) (int, error) {
	have := map[string]bool{}
	for _, it := range items {
		have[it.ID] = true
	}
	added, dups, moved, lost := 0, 0, 0, 0
	for _, f := range files {
		var b []byte
		var err error
		if f == "-" {
			b, err = io.ReadAll(stdin)
			f = "standard input"
		} else {
			b, err = os.ReadFile(f)
		}
		if err != nil {
			return 2, err
		}
		in, err := comments.ParseExport(f, b)
		if err != nil {
			return 2, err
		}
		for _, it := range in {
			if have[it.ID] {
				dups++
				continue
			}
			have[it.ID] = true
			path, _, state := comments.Locate(rootOf(p, it), it)
			switch state {
			case comments.Moved:
				it.Path = path
				moved++
			case comments.Changed, comments.Removed, comments.Missing:
				lost++
			}
			it.Status, it.Internal, it.Reply = comments.Open, internal, ""
			items = append(items, it)
			added++
		}
	}
	if err := comments.Save(root, items); err != nil {
		return 2, err
	}
	if c.format == "json" {
		return 0, writeJSON(stdout, map[string]int{"imported": added, "duplicates": dups, "moved": moved, "changed": lost})
	}
	say(stdout, "imported %d comments into %s", added, comments.File)
	var notes []string
	if dups > 0 {
		notes = append(notes, fmt.Sprintf("%d already there", dups))
	}
	if moved > 0 {
		notes = append(notes, fmt.Sprintf("%d attached to their moved block", moved))
	}
	if lost > 0 {
		notes = append(notes, fmt.Sprintf("%d on a block that changed or is gone", lost))
	}
	if len(notes) > 0 {
		say(stdout, " (%s)", strings.Join(notes, ", "))
	}
	say(stdout, "\n")
	return 0, nil
}

// find returns the index of the comment with the given id or unique id prefix.
func find(items []comments.Item, id string) (int, error) {
	at := -1
	for i, it := range items {
		if it.ID == id {
			return i, nil
		}
		if strings.HasPrefix(it.ID, id) {
			if at >= 0 {
				return -1, usageErr("%q matches more than one comment: give more characters", id)
			}
			at = i
		}
	}
	if at < 0 {
		return -1, usageErr("no comment %q in %s", id, comments.File)
	}
	return at, nil
}

func changeComments(c *common, root string, items []comments.Item, ids []string, sub, reply string, stdout io.Writer) (int, error) {
	drop := map[int]bool{}
	var done []string
	for _, id := range ids {
		i, err := find(items, id)
		if err != nil {
			return 2, err
		}
		if sub == "done" {
			items[i].Status = comments.Done
			if reply != "" {
				items[i].Reply = reply
			}
		} else {
			drop[i] = true
		}
		done = append(done, items[i].ID)
	}
	var keep []comments.Item
	for i, it := range items {
		if !drop[i] {
			keep = append(keep, it)
		}
	}
	if err := comments.Save(root, keep); err != nil {
		return 2, err
	}
	verb := map[string]string{"done": "processed", "rm": "deleted"}[sub]
	if c.format == "json" {
		return 0, writeJSON(stdout, map[string]any{verb: done})
	}
	for _, id := range done {
		say(stdout, "%s %s\n", verb, short(id))
	}
	return 0, nil
}

func short(id string) string {
	if len(id) > 8 {
		return id[:8]
	}
	return id
}

type listed struct {
	comments.Item
	Title   string `json:"title,omitempty"`
	Current string `json:"current"`
	State   string `json:"state"` // unchanged, moved, changed, removed (the block), missing (the document)
}

func listComments(c *common, p *engine.Project, items []comments.Item, all bool, stdout io.Writer) (int, error) {
	var out []listed
	open := 0
	for _, it := range items {
		if it.Status != comments.Done {
			open++
		} else if !all {
			continue
		}
		l := listed{Item: it}
		var text string
		l.Path, text, l.State = comments.Locate(rootOf(p, it), it)
		l.Current = text
		if d := p.Graph[it.Doc]; d != nil && d.Root != nil {
			if t := d.Root.Get("title"); t != nil && t.Kind == loader.String {
				l.Title = t.Str
			}
		}
		out = append(out, l)
	}
	if c.format == "json" {
		if out == nil {
			out = []listed{}
		}
		return 0, writeJSON(stdout, map[string]any{"open": open, "total": len(items), "comments": out})
	}
	for _, l := range out {
		kind := ""
		if l.Internal {
			kind = " internal"
		}
		say(stdout, "%s  %s%s  %s %q %s\n", short(l.ID), statusWord(l.Status), kind, l.Doc, l.Title, l.Path)
		who := l.Author
		if l.Created != "" {
			who = strings.TrimSpace(who + " " + l.Created[:min(10, len(l.Created))])
		}
		if who != "" {
			who += ": "
		}
		if l.Fragment != "" {
			say(stdout, "  about: «%s»\n", indent(l.Fragment))
		}
		say(stdout, "  %s%s\n", who, indent(l.Text))
		if l.Reply != "" {
			say(stdout, "  reply: %s\n", indent(l.Reply))
		}
		switch l.State {
		case comments.Unchanged, comments.Moved:
			say(stdout, "  block: %s\n", indent(l.Quote))
		case comments.Changed:
			say(stdout, "  block when commented: %s\n  block now: %s\n", indent(l.Quote), indent(l.Current))
		case comments.Removed:
			say(stdout, "  block is gone; it read: %s\n", indent(l.Quote))
		case comments.Missing:
			say(stdout, "  document %s is gone; the block read: %s\n", l.Doc, indent(l.Quote))
		}
		say(stdout, "\n")
	}
	say(stdout, "%d not processed, %d in all (%s)\n", open, len(items), comments.File)
	return 0, nil
}

func statusWord(s string) string {
	if s == comments.Done {
		return "done"
	}
	return "open"
}
