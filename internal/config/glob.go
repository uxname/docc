package config

import (
	"regexp"
	"strings"
)

// Glob is a compiled path mask: `*` does not cross '/', `**` does.
// `**/` also matches zero directories.
type Glob struct{ re *regexp.Regexp }

func CompileGlob(mask string) (*Glob, error) {
	var b strings.Builder
	b.WriteString("^")
	for i := 0; i < len(mask); {
		switch {
		case strings.HasPrefix(mask[i:], "**/"):
			b.WriteString("(?:.*/)?")
			i += 3
		case strings.HasPrefix(mask[i:], "**"):
			b.WriteString(".*")
			i += 2
		case mask[i] == '*':
			b.WriteString("[^/]*")
			i++
		case mask[i] == '?':
			b.WriteString("[^/]")
			i++
		default:
			b.WriteString(regexp.QuoteMeta(mask[i : i+1]))
			i++
		}
	}
	b.WriteString("$")
	re, err := regexp.Compile(b.String())
	if err != nil {
		return nil, err
	}
	return &Glob{re}, nil
}

func (g *Glob) Match(path string) bool { return g.re.MatchString(path) }

// MatchAny reports whether path matches at least one glob.
func MatchAny(gs []*Glob, path string) bool {
	for _, g := range gs {
		if g.Match(path) {
			return true
		}
	}
	return false
}
