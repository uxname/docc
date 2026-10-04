package main

import (
	"encoding/json"
	"flag"
	"io"
	"path/filepath"
	"strings"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/rules"
)

type ruleRow struct {
	Code  string `json:"code"`
	Name  string `json:"name"`
	Group string `json:"group"`
	Level string `json:"level"`
	Fix   string `json:"fix"`
}

func writeJSON(w io.Writer, v any) error {
	b, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	_, err = w.Write(append(b, '\n'))
	return err
}

// relToSrc turns a path given to --file into a path relative to src.
func relToSrc(cfg *config.Config, file string) string {
	f := filepath.ToSlash(filepath.Clean(file))
	if filepath.IsAbs(file) {
		if rel, err := filepath.Rel(cfg.Root, file); err == nil {
			f = filepath.ToSlash(rel)
		}
	}
	return strings.TrimPrefix(f, cfg.Src+"/")
}

func cmdRules(args []string, stdout io.Writer) (int, error) {
	var c common
	fs := newFlags("rules", &c)
	file := fs.String("file", "", "show levels for this file (overrides applied)")
	pos, err := parse(fs, args)
	if err == nil {
		err = noArgs(pos)
	}
	if err == nil {
		err = c.check()
	}
	if err != nil {
		return 2, err
	}
	cfg, err := loadConfig(&c)
	if err != nil {
		return 2, err
	}
	rel := ""
	if *file != "" {
		rel = relToSrc(cfg, *file)
	}
	res := cfg.For(rel)
	var rows []ruleRow
	for _, r := range rules.All() {
		if r.Code == "DOC800" {
			continue // custom rules are listed individually
		}
		rows = append(rows, ruleRow{r.Code, r.Name, r.Group, res.Rule(r.Code).Level.String(), r.Fix})
	}
	for _, cr := range cfg.Custom {
		rows = append(rows, ruleRow{"DOC800", cr.ID, "custom", res.Custom(cr.ID).Level.String(), "none"})
	}
	if c.format == "json" {
		return 0, writeJSON(stdout, rows)
	}
	return 0, writeTable(stdout, rows)
}

func writeTable(w io.Writer, rows []ruleRow) error {
	head := ruleRow{"CODE", "RULE", "GROUP", "LEVEL", "FIX"}
	all := append([]ruleRow{head}, rows...)
	var wd [4]int
	for _, r := range all {
		for i, s := range []string{r.Code, r.Name, r.Group, r.Level} {
			wd[i] = max(wd[i], len([]rune(s)))
		}
	}
	var b strings.Builder
	pad := func(s string, n int) string { return s + strings.Repeat(" ", n-len([]rune(s))) }
	for _, r := range all {
		b.WriteString(pad(r.Code, wd[0]) + "  " + pad(r.Name, wd[1]) + "  " + pad(r.Group, wd[2]) + "  " + pad(r.Level, wd[3]) + "  " + r.Fix + "\n")
	}
	_, err := io.WriteString(w, b.String())
	return err
}

type explanation struct {
	Code           string `json:"code"`
	Name           string `json:"name"`
	Group          string `json:"group"`
	Level          string `json:"level"`
	Description    string `json:"description"`
	Bad            string `json:"bad"`
	Good           string `json:"good"`
	Fix            string `json:"fix"`
	FixDescription string `json:"fixDescription"`
}

func cmdExplain(args []string, stdout io.Writer) (int, error) {
	var c common
	fs := flag.NewFlagSet("explain", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	fs.StringVar(&c.format, "format", "text", "text or json")
	fs.StringVar(&c.configPath, "config", "", "path to docc.json (for custom rules)")
	pos, err := parse(fs, args)
	if err == nil && len(pos) != 1 {
		err = usageErr("usage: docc explain <code or rule name>")
	}
	if err == nil {
		err = c.check()
	}
	if err != nil {
		return 2, err
	}
	e, err := explain(&c, pos[0])
	if err != nil {
		return 2, err
	}
	if c.format == "json" {
		return 0, writeJSON(stdout, e)
	}
	fix := "none"
	if e.Fix != "none" {
		fix = e.Fix + ": " + e.FixDescription
	}
	text := e.Code + " " + e.Name + "\nGroup: " + e.Group + "\nDefault level: " + e.Level + "\n\n" +
		e.Description + "\n\nViolation:\n  " + indent(e.Bad) + "\n\nFixed:\n  " + indent(e.Good) + "\n\nAutomatic fix: " + fix + "\n"
	_, err = io.WriteString(stdout, text)
	return 0, err
}

func indent(s string) string { return strings.ReplaceAll(s, "\n", "\n  ") }

func explain(c *common, key string) (*explanation, error) {
	if r := rules.Lookup(key); r != nil {
		return &explanation{r.Code, r.Name, r.Group, r.Default.String(), r.Desc, r.Bad, r.Good, r.Fix, r.FixDesc}, nil
	}
	if rules.IsCustomName(key) {
		cfg, err := loadConfig(c)
		if err != nil {
			return nil, err
		}
		for _, cr := range cfg.Custom {
			if cr.ID != key {
				continue
			}
			kind, ok := "a value that matches "+cr.Forbid.String(), "a value without a match"
			if cr.Require != nil {
				kind, ok = "a value that does not match "+cr.Require.String(), "a value that matches the required expression"
			}
			desc := cr.Message + " (custom rule from " + cfg.File + ", checks " + cr.Given + ")"
			if cr.Hint != "" {
				ok = cr.Hint
			}
			return &explanation{"DOC800", cr.ID, "custom", cr.Level.String(), desc, kind, ok, "none", ""}, nil
		}
	}
	return nil, usageErr("unknown rule %q: use a code like DOC301 or a name like text/forbidden-words", key)
}
