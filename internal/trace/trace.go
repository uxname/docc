// Package trace finds docc: tags in code files.
package trace

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"

	"golang.org/x/text/unicode/norm"

	"github.com/uxname/docc/internal/config"
)

const maxFileSize = 1 << 20

// Tag is one "docc:" tag, or the opening "docc:begin" line of a region.
type Tag struct {
	File      string // path relative to the project root, with '/'
	Line, Col int
	Targets   []string // as written: id, id#field[/section] or id#^anchor
	Region    bool     // docc:begin ... docc:end
	Body      string   // region lines between the markers, trimmed, NFC, joined with \n
	Err       string   // a malformed region: unclosed, nested or a stray end
}

// Key is a pair of a code file and a target.
type Key struct{ File, Target string }

// Hashes returns, per file and target, the hex SHA-256 of the bodies of all
// closed regions with that target, in file order. A plain tag has no hash.
func Hashes(tags []Tag) map[Key]string {
	bodies := map[Key][]string{}
	for _, t := range tags {
		if t.Region && t.Err == "" {
			for _, g := range t.Targets {
				k := Key{t.File, g}
				bodies[k] = append(bodies[k], t.Body)
			}
		}
	}
	out := map[Key]string{}
	for k, b := range bodies {
		sum := sha256.Sum256([]byte(strings.Join(b, "\x00"))) // NUL never occurs in a scanned file
		out[k] = hex.EncodeToString(sum[:])
	}
	return out
}

// Scan searches the files that match masks (relative to root). Inside a git
// work tree the files are those of `git ls-files -co --exclude-standard`;
// otherwise the tree is walked without .git, node_modules and vendor.
// Skipped always: the given directories (docc's own src, out and outFull),
// symbolic links, files over 1 MB and files with a NUL byte.
func Scan(root string, masks []*config.Glob, skipDirs []string) ([]Tag, error) {
	paths, err := list(root, skipDirs)
	if err != nil {
		return nil, err
	}
	var tags []Tag
	for _, rel := range paths {
		if !config.MatchAny(masks, rel) {
			continue
		}
		abs := filepath.Join(root, filepath.FromSlash(rel))
		if st, err := os.Lstat(abs); err != nil || !st.Mode().IsRegular() || st.Size() > maxFileSize {
			continue
		}
		data, err := os.ReadFile(abs)
		if err != nil || bytes.IndexByte(data, 0) >= 0 {
			continue
		}
		tags = append(tags, scanFile(rel, bytes.TrimPrefix(data, []byte{0xEF, 0xBB, 0xBF}))...)
	}
	return tags, nil
}

// list returns the candidate files, relative to root with '/', sorted.
func list(root string, skipDirs []string) ([]string, error) {
	under := func(abs string) bool {
		for _, d := range skipDirs {
			if r, err := filepath.Rel(d, abs); err == nil && r != ".." && !strings.HasPrefix(r, ".."+string(filepath.Separator)) {
				return true
			}
		}
		return false
	}
	var paths []string
	cmd := exec.Command("git", "ls-files", "-co", "--exclude-standard", "-z")
	cmd.Dir = root
	if out, err := cmd.Output(); err == nil {
		for _, p := range strings.Split(string(out), "\x00") {
			if p != "" && !under(filepath.Join(root, filepath.FromSlash(p))) {
				paths = append(paths, p)
			}
		}
		slices.Sort(paths)
		return slices.Compact(paths), nil // unmerged files are listed once per stage
	}
	err := filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil // unreadable entries are skipped
		}
		if d.IsDir() {
			switch {
			case p == root:
			case d.Name() == ".git" || d.Name() == "node_modules" || d.Name() == "vendor" || under(p):
				return filepath.SkipDir
			}
			return nil
		}
		rel, _ := filepath.Rel(root, p)
		paths = append(paths, filepath.ToSlash(rel))
		return nil
	})
	slices.Sort(paths)
	return paths, err
}

// markers are the comment openers a tag may follow; longer ones first.
var markers = []string{"<!--", "/**", "/*", "//", "--", "#", "*", ";", "%", "'"}

// commentOnly reports whether s holds only blanks and comment markers.
func commentOnly(s string) bool {
	for s = strings.TrimSpace(s); s != ""; s = strings.TrimSpace(s) {
		i := slices.IndexFunc(markers, func(m string) bool { return strings.HasPrefix(s, m) })
		if i < 0 {
			return false
		}
		s = s[len(markers[i]):]
	}
	return true
}

// targets splits "a, b#x */" into its targets; comment closers are dropped.
func targets(s string) []string {
	s = strings.TrimSpace(s)
	for _, c := range []string{"*/", "-->"} {
		s = strings.TrimSpace(strings.TrimSuffix(s, c))
	}
	if s == "" {
		return nil
	}
	var out []string
	for _, t := range strings.Split(s, ",") {
		out = append(out, strings.TrimSpace(t))
	}
	return out
}

func scanFile(rel string, data []byte) []Tag {
	var tags []Tag
	open := -1 // index in tags of the open region
	var body []string
	lines := strings.Split(string(data), "\n")
	for i, line := range lines {
		line = strings.TrimSuffix(line, "\r")
		at := strings.Index(line, "docc:")
		if at < 0 || !commentOnly(line[:at]) {
			if open >= 0 {
				body = append(body, norm.NFC.String(strings.TrimSpace(line)))
			}
			continue
		}
		t := Tag{File: rel, Line: i + 1, Col: utf8.RuneCountInString(line[:at]) + 1}
		rest := line[at+len("docc:"):]
		word, args := rest, ""
		if j := strings.IndexFunc(rest, unicode.IsSpace); j >= 0 {
			word, args = rest[:j], rest[j:]
		}
		word = strings.TrimSpace(strings.TrimSuffix(strings.TrimSuffix(word, "*/"), "-->"))
		switch word {
		case "begin":
			t.Region, t.Targets = true, targets(args)
			if open >= 0 {
				t.Err = "a region opened inside another region"
			} else {
				open, body = len(tags), nil
			}
		case "end":
			if open < 0 {
				t.Err = "docc:end without docc:begin"
			} else {
				tags[open].Body = strings.Join(body, "\n")
				open = -1
				continue
			}
		default:
			t.Targets = targets(rest)
			if open >= 0 {
				body = append(body, norm.NFC.String(strings.TrimSpace(line)))
			}
		}
		tags = append(tags, t)
	}
	if open >= 0 {
		tags[open].Err = "docc:begin without docc:end"
	}
	return tags
}
