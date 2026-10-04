package main

import (
	"encoding/json"
	"io"
	"path/filepath"
	"strings"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/rules"
)

type ruleRow struct {
	Code        string `json:"code"`
	Rule        string `json:"rule"`
	Group       string `json:"group"`
	Level       string `json:"level"`
	Fixable     string `json:"fixable"`
	Description string `json:"description"`
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

func cmdRules(c *common, args []string, stdout io.Writer) (int, error) {
	fs := newFlags("rules", c)
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
	cfg, err := loadConfig(c)
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
		if r.Code == "DOC800" { // custom rules are listed individually, in its place
			for _, cr := range cfg.Custom {
				rows = append(rows, ruleRow{"DOC800", cr.ID, "custom", res.Custom(cr.ID).Level.String(), "none", cr.Message})
			}
			continue
		}
		rows = append(rows, ruleRow{r.Code, r.Name, r.Group, res.Rule(r.Code).Level.String(), r.Fix, r.Desc})
	}
	if c.format == "json" {
		return 0, writeJSON(stdout, rows)
	}
	return 0, writeTable(stdout, rows)
}

// firstSentence shortens a description for the table.
func firstSentence(s string) string {
	if i := strings.Index(s, ". "); i >= 0 {
		return s[:i+1]
	}
	return s
}

func writeTable(w io.Writer, rows []ruleRow) error {
	head := ruleRow{"CODE", "RULE", "GROUP", "LEVEL", "FIXABLE", "DESCRIPTION"}
	all := append([]ruleRow{head}, rows...)
	var wd [5]int
	for _, r := range all {
		for i, s := range []string{r.Code, r.Rule, r.Group, r.Level, r.Fixable} {
			wd[i] = max(wd[i], len([]rune(s)))
		}
	}
	var b strings.Builder
	pad := func(s string, n int) string { return s + strings.Repeat(" ", n-len([]rune(s))) }
	for _, r := range all {
		b.WriteString(pad(r.Code, wd[0]) + "  " + pad(r.Rule, wd[1]) + "  " + pad(r.Group, wd[2]) + "  " + pad(r.Level, wd[3]) + "  " + pad(r.Fixable, wd[4]) + "  " + firstSentence(r.Description) + "\n")
	}
	_, err := io.WriteString(w, b.String())
	return err
}

type explanation struct {
	Code           string `json:"code"`
	Rule           string `json:"rule"`
	Group          string `json:"group"`
	Level          string `json:"level"`
	Description    string `json:"description"`
	Bad            string `json:"bad"`
	Good           string `json:"good"`
	Fixable        string `json:"fixable"`
	FixDescription string `json:"fixDescription,omitempty"`

	core bool
	def  string // the default level, when Level is overridden by the config
}

func cmdExplain(c *common, args []string, stdout io.Writer) (int, error) {
	fs := newFlags("explain", c)
	file := fs.String("file", "", "show the level for this file (overrides applied)")
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
	e, err := explain(c, pos[0], *file)
	if err != nil {
		return 2, err
	}
	if c.format == "json" {
		return 0, writeJSON(stdout, e)
	}
	fix := "none"
	if e.Fixable != "none" {
		fix = e.Fixable + ": " + e.FixDescription
	}
	level := e.Level
	if e.def != "" {
		level += " (default: " + e.def + ")"
	}
	text := e.Code + " " + e.Rule + "\nGroup: " + e.Group + "\nLevel: " + level + "\n\n" +
		e.Description + "\n\nViolation:\n  " + indent(e.Bad) + "\n\nFixed:\n  " + indent(e.Good) + "\n\nAutomatic fix: " + fix + "\n\n" +
		configure(e) + "\n"
	_, err = io.WriteString(stdout, text)
	return 0, err
}

func indent(s string) string { return strings.ReplaceAll(s, "\n", "\n  ") }

// configure shows how to change the level of a rule or suppress one finding.
func configure(e *explanation) string {
	if e.core {
		return "Configure: this rule cannot be disabled, re-levelled or suppressed."
	}
	key := e.Code
	if e.Code == "DOC800" {
		key = e.Rule
	}
	return "Configure (docc.json):\n  \"rules\": { \"" + key + "\": \"off\" }   (or \"warn\", \"error\")\n" +
		"Suppress one finding (in the document):\n  \"docIgnore\": [{ \"rule\": \"" + key + "\", \"reason\": \"why this is fine\" }]"
}

func explain(c *common, key, file string) (*explanation, error) {
	cfg, err := loadConfig(c)
	if r := rules.Lookup(key); r != nil {
		e := &explanation{Code: r.Code, Rule: r.Name, Group: r.Group, Level: r.Default.String(), Description: r.Desc, Bad: r.Bad, Good: r.Good, Fixable: r.Fix, FixDescription: r.FixDesc, core: r.Core}
		if err == nil && !r.Core && r.Code != "DOC800" { // a broken config must not stop a reference lookup
			rel := ""
			if file != "" {
				rel = relToSrc(cfg, file)
			}
			if l := cfg.For(rel).Rule(r.Code).Level.String(); l != e.Level {
				e.def, e.Level = e.Level, l
			}
		}
		return e, nil
	}
	if rules.IsCustomName(key) {
		if err != nil {
			return nil, err
		}
		for _, cr := range cfg.Custom {
			if cr.ID != key {
				continue
			}
			var kind, ok string
			if cr.Require != nil {
				kind, ok = "a value that does not match "+cr.Require.String(), "a value that matches the required expression"
			} else {
				kind, ok = "a value that matches "+cr.Forbid.String(), "a value without a match"
			}
			desc := cr.Message + " (custom rule from " + cfg.File + ", checks " + cr.Given + ")"
			if cr.Hint != "" {
				ok = cr.Hint
			}
			rel := ""
			if file != "" {
				rel = relToSrc(cfg, file)
			}
			return &explanation{Code: "DOC800", Rule: cr.ID, Group: "custom", Level: cfg.For(rel).Custom(cr.ID).Level.String(), Description: desc, Bad: kind, Good: ok, Fixable: "none"}, nil
		}
	}
	return nil, usageErr("unknown rule %q: use a code like DOC301 or a name like text/forbidden-words", key)
}
