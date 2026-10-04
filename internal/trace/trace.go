// Package trace finds @covers tags in code files.
package trace

import (
	"bytes"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/santiment/docc/internal/config"
)

const maxFileSize = 1 << 20

// Tag is one @covers occurrence: the position of the tag and the ids after it.
type Tag struct {
	File      string // path relative to the project root, with '/'
	Line, Col int
	IDs       []string
	Next      string // the word right after the ids on the same line, if separated by blanks only
}

var tagRe = regexp.MustCompile(`@covers[ \t]+([a-z0-9-]+(?:[ \t]*,[ \t]*[a-z0-9-]+)*)`)

var nextRe = regexp.MustCompile(`^[a-z0-9-]+`)

// Scan searches the files that match masks (relative to root). Skipped:
// .git, the given directories (docc's own src, out and outFull), symbolic
// links, files over 1 MB and files with a NUL byte.
func Scan(root string, masks []*config.Glob, skipDirs []string) ([]Tag, error) {
	skip := map[string]bool{}
	for _, d := range skipDirs {
		skip[filepath.Clean(d)] = true
	}
	var paths []string
	err := filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil // unreadable entries are skipped
		}
		if d.Type()&fs.ModeSymlink != 0 {
			return nil
		}
		if d.IsDir() {
			if d.Name() == ".git" || skip[filepath.Clean(p)] {
				return filepath.SkipDir
			}
			return nil
		}
		rel, _ := filepath.Rel(root, p)
		rel = filepath.ToSlash(rel)
		if d.Type().IsRegular() && config.MatchAny(masks, rel) {
			paths = append(paths, rel)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Strings(paths)
	var tags []Tag
	for _, rel := range paths {
		abs := filepath.Join(root, filepath.FromSlash(rel))
		if st, err := os.Stat(abs); err != nil || st.Size() > maxFileSize {
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

func scanFile(rel string, data []byte) []Tag {
	var tags []Tag
	for i, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSuffix(line, "\r")
		for _, m := range tagRe.FindAllStringSubmatchIndex(line, -1) {
			var ids []string
			for _, id := range strings.Split(line[m[2]:m[3]], ",") {
				ids = append(ids, strings.TrimSpace(id))
			}
			next := ""
			if rest := line[m[1]:]; len(rest) > 0 && (rest[0] == ' ' || rest[0] == '\t') {
				next = nextRe.FindString(strings.TrimLeft(rest, " \t"))
			}
			tags = append(tags, Tag{File: rel, Line: i + 1, Col: utf8.RuneCountInString(line[:m[0]]) + 1, IDs: ids, Next: next})
		}
	}
	return tags
}
