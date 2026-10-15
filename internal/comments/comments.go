// Package comments keeps the review comments left on blocks of the HTML file:
// the export the browser downloads and docc.comments.json in the project root.
package comments

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/uxname/docc/internal/history"
	"github.com/uxname/docc/internal/loader"
	"github.com/uxname/docc/internal/schema"
)

// File is the name of the comments file in the project root.
const File = "docc.comments.json"

const (
	Open = "open"
	Done = "done"
)

// Item is one comment on a block: the document id, the JSON Pointer of the
// block and the text of the block when the comment was written.
type Item struct {
	ID    string `json:"id"`
	Doc   string `json:"doc"`
	Path  string `json:"path"`
	Quote string `json:"quote"`
	// Fragment is the part of the block the reader selected, when the
	// comment is about a few words rather than the whole block.
	Fragment string `json:"fragment,omitempty"`
	// Prefix and Suffix are a few characters around the fragment, to tell
	// one occurrence of it from another.
	Prefix  string `json:"prefix,omitempty"`
	Suffix  string `json:"suffix,omitempty"`
	Text    string `json:"text"`
	Author  string `json:"author,omitempty"`
	Created string `json:"created,omitempty"`
	Build   string `json:"build,omitempty"`
	Status  string `json:"status,omitempty"`
	// Reply is the team's answer, shown under the comment in the next HTML file.
	Reply string `json:"reply,omitempty"`
	// Internal comments (imported with --internal) stay out of the HTML file.
	Internal bool `json:"internal,omitempty"`
}

// Export is the file the HTML page downloads.
type Export struct {
	Docc     string `json:"docc"`
	Version  int    `json:"version"`
	Comments []Item `json:"comments"`
}

type store struct {
	Comments []Item `json:"comments"`
}

// Load reads docc.comments.json from the project root; a missing file is an
// empty list.
func Load(root string) ([]Item, error) {
	b, err := os.ReadFile(filepath.Join(root, File))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var s store
	if err := json.Unmarshal(b, &s); err != nil {
		return nil, fmt.Errorf("%s: %v", File, err)
	}
	return s.Comments, nil
}

// Save writes the comments to docc.comments.json, one comment per line, so a
// change of status is a one-line diff.
func Save(root string, items []Item) error {
	var b bytes.Buffer
	b.WriteString("{\"comments\": [")
	for i, it := range items {
		line, err := marshal(it)
		if err != nil {
			return err
		}
		if i > 0 {
			b.WriteByte(',')
		}
		b.WriteString("\n  ")
		b.Write(line)
	}
	if len(items) > 0 {
		b.WriteString("\n")
	}
	b.WriteString("]}\n")
	return os.WriteFile(filepath.Join(root, File), b.Bytes(), 0o644)
}

func marshal(v any) ([]byte, error) {
	var b bytes.Buffer
	enc := json.NewEncoder(&b)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimSuffix(b.Bytes(), []byte("\n")), nil
}

// Marker starts the machine part of the comments copied from the HTML page
// as text: the export JSON in base64, so a messenger cannot mangle it.
const Marker = "docc-comments:"

// the base64 may be folded into lines by a messenger or mail, so it runs to the end
var markerRe = regexp.MustCompile(regexp.QuoteMeta(Marker) + `([A-Za-z0-9+/=\s]+)`)

// ParseExport reads what the HTML page gives away: the downloaded file, or
// the copied text with a Marker line at its end (the last one counts: a
// comment may quote the marker). name is for errors.
func ParseExport(name string, b []byte) ([]Item, error) {
	if all := markerRe.FindAllSubmatch(b, -1); all != nil {
		raw, err := base64.StdEncoding.DecodeString(strings.Join(strings.Fields(string(all[len(all)-1][1])), ""))
		if err != nil {
			return nil, fmt.Errorf("%s: the %s line is damaged: copy the comments again", name, Marker)
		}
		b = raw
	} else if t := bytes.TrimSpace(b); len(t) == 0 || t[0] != '{' {
		return nil, fmt.Errorf("%s: no comments in it: give the file downloaded from the page or the text copied there (it has a %s line)", name, Marker)
	}
	var e Export
	if err := json.Unmarshal(b, &e); err != nil {
		return nil, fmt.Errorf("%s: %v", name, err)
	}
	path := name
	if e.Docc != "comments" {
		return nil, fmt.Errorf("%s: not a comments file of docc (no \"docc\": \"comments\")", path)
	}
	if e.Version != 1 {
		return nil, fmt.Errorf("%s: unsupported version %d", path, e.Version)
	}
	for i, it := range e.Comments {
		if it.ID == "" || it.Doc == "" || it.Text == "" {
			return nil, fmt.Errorf("%s: comment %d lacks id, doc or text", path, i+1)
		}
	}
	return e.Comments, nil
}

// States of a comment's block, see Locate.
const (
	Unchanged = "unchanged" // the block still reads as quoted
	Moved     = "moved"     // the quoted text stands at another pointer now
	Changed   = "changed"   // the block was edited
	Removed   = "removed"   // no block at the pointer
	Missing   = "missing"   // no such document
)

// Locate finds the block of a comment in the current document (root nil: the
// document is gone): the pointer to use, its current text and the state.
func Locate(root *loader.Node, it Item) (path, text, state string) {
	if root == nil {
		return it.Path, "", Missing
	}
	if n := schema.ResolvePointer(root, it.Path); n != nil && it.Path != "" {
		text = history.Text(n)
		if text == it.Quote {
			return it.Path, text, Unchanged
		}
	}
	leaves := history.Flatten(root)
	for _, l := range leaves {
		if l.Text == it.Quote {
			return l.Ptr, l.Text, Moved
		}
	}
	// edited: the block at the pointer if it still resembles the quote, else the most similar one
	if text != "" && history.Similar(text, it.Quote) >= 0.5 {
		return it.Path, text, Changed
	}
	best, bp, bt := 0.3, "", ""
	for _, l := range leaves {
		if s := history.Similar(l.Text, it.Quote); s > best || (s == best && l.Ptr == it.Path) {
			best, bp, bt = s, l.Ptr, l.Text
		}
	}
	if bp != "" {
		return bp, bt, Changed
	}
	return it.Path, "", Removed
}
