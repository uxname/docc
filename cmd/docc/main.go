// Command docc is a compiler and linter for JSON documentation.
package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime/debug"
	"strings"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/fixes"
	"github.com/santiment/docc/internal/render"
	"github.com/santiment/docc/internal/report"
	"github.com/santiment/docc/internal/schema"
)

var version = "dev"

const usage = `docc - compiler and linter for JSON documentation

Usage: docc [flags] <command> [flags]

Commands:
  init               create docc.json and docs/src/index.json, build docs/out
  validate           check sources (--fix applies safe fixes)
  build              build Markdown into docs/out (--full: audit, --single/--html: one file)
  check              read-only check for CI: validate + freshness of out/
  explain <code>     describe a rule
  rules              list rules with effective levels
  schema document|config   print a JSON Schema

Flags: --format text|json, --config <file>, --max-diagnostics N, --max-warnings N
(they may stand before or after the command)

Run docc <command> --help for the flags and examples of a command.
`

// cmdUsage is printed by docc <command> --help, before the flag list.
var cmdUsage = map[string]string{
	"init": `Usage: docc init

Creates docc.json and docs/src/index.json (existing files are kept) and builds docs/out.

Example:
  docc init
`,
	"validate": `Usage: docc validate [flags]

Checks the sources in docs/src and prints diagnostics. Exit code 1: errors, or more
warnings than --max-warnings.

Examples:
  docc validate --format json
  docc validate --fix
`,
	"build": `Usage: docc build [flags]

Validates and writes Markdown into docs/out. With errors in groups 1xx-2xx nothing is written.
--single also writes every page into one Markdown file next to the directory (docs/out.md), --html
into one self-contained HTML file (docs/out.html): a document tree, search, dark theme and print
styles, no network needed. With --full the files are docs/out-full.md and docs/out-full.html. They
are for reading, are not checked by docc check and are usually kept out of git.

Examples:
  docc build
  docc build --full
  docc build --single
  docc build --html
  docc build --full --single --html
`,
	"check": `Usage: docc check [flags]

Read-only check for CI: validate, and fail (DOC501) when docs/out differs from the sources.

Example:
  docc check --max-warnings 0
`,
	"explain": `Usage: docc explain <code or rule name> [flags]

Describes a rule: what it checks, bad and good examples, how to configure it.

Examples:
  docc explain DOC301
  docc explain text/forbidden-words --format json
`,
	"rules": `Usage: docc rules [flags]

Lists every rule with its effective level.

Examples:
  docc rules
  docc rules --file docs/src/index.json
`,
	"schema": `Usage: docc schema document|config

Prints a JSON Schema for documents or for docc.json.

Example:
  docc schema document
`,
}

// helpError makes run print text to stdout and exit with code 0.
type helpError struct{ text string }

func (e *helpError) Error() string { return "help requested" }

// buildVersion is the release version, or the VCS version of a go install build.
func buildVersion() string {
	if version == "dev" {
		if info, ok := debug.ReadBuildInfo(); ok && info.Main.Version != "" && info.Main.Version != "(devel)" {
			return info.Main.Version
		}
	}
	return version
}

// say writes to a stream; a failed write to stdout or stderr has nowhere to go.
func say(w io.Writer, format string, a ...any) { _, _ = fmt.Fprintf(w, format, a...) }

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

// usageError makes run exit with code 2 and a message on stderr.
type usageError struct{ msg string }

func (e *usageError) Error() string { return e.msg }

func usageErr(format string, a ...any) error { return &usageError{fmt.Sprintf(format, a...)} }

func run(args []string, stdout, stderr io.Writer) int {
	if len(args) == 0 {
		say(stderr, "%s", usage)
		return 2
	}
	switch args[0] {
	case "--version", "-version", "version":
		say(stdout, "docc %s\n", buildVersion())
		return 0
	case "--help", "-h", "help":
		say(stdout, "%s", usage)
		return 0
	}
	c := common{format: "text", maxDiag: 20, maxWarnings: -1}
	code, err := dispatch(&c, args, stdout, stderr)
	if err == nil {
		return code
	}
	var he *helpError
	if errors.As(err, &he) {
		say(stdout, "%s", he.text)
		return 0
	}
	if c.format == "json" {
		if werr := writeJSON(stdout, errorObject(err)); werr != nil {
			say(stderr, "docc: error: %v\n", werr)
		}
	} else {
		say(stderr, "docc: error: %s\n", err)
	}
	return 2
}

// errorObject is the JSON form of a launch or config error.
func errorObject(err error) map[string]any {
	o := map[string]any{"code": "error", "message": err.Error(), "hint": "", "file": "", "line": 0, "column": 0}
	var ue *usageError
	var eu *engine.UsageError
	var ce *config.Error
	switch {
	case errors.As(err, &ce):
		o["code"], o["message"], o["file"], o["line"], o["column"] = "config", ce.Msg, ce.File, ce.Line, ce.Col
	case errors.As(err, &ue), errors.As(err, &eu):
		o["code"], o["hint"] = "usage", "run docc --help"
	}
	return o
}

// dispatch takes the global flags that stand before the command, then runs it.
func dispatch(c *common, args []string, stdout, stderr io.Writer) (int, error) {
	gf := newFlags("docc", c)
	if err := gf.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0, &helpError{usage}
		}
		return 2, usageErr("%v", err)
	}
	args = gf.Args()
	if len(args) == 0 {
		return 2, usageErr("no command given (try docc --help)")
	}
	switch args[0] {
	case "validate":
		return cmdValidate(c, args[1:], stdout, stderr)
	case "build":
		return cmdBuild(c, args[1:], stdout, stderr)
	case "check":
		return cmdCheck(c, args[1:], stdout, stderr)
	case "init":
		return cmdInit(c, args[1:], stdout, stderr)
	case "rules":
		return cmdRules(c, args[1:], stdout)
	case "explain":
		return cmdExplain(c, args[1:], stdout)
	case "schema":
		return 0, cmdSchema(args[1:], stdout)
	}
	return 2, usageErr("unknown command %q (try docc --help)", args[0])
}

// common flags shared by the checking commands.
type common struct {
	format      string
	configPath  string
	maxDiag     int
	maxWarnings int
}

func newFlags(name string, c *common) *flag.FlagSet {
	fs := flag.NewFlagSet(name, flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	// the defaults are the current values: global flags may be given before the command
	fs.StringVar(&c.format, "format", c.format, "text or json")
	fs.StringVar(&c.configPath, "config", c.configPath, "path to docc.json")
	fs.IntVar(&c.maxDiag, "max-diagnostics", c.maxDiag, "output limit (0: no limit)")
	fs.IntVar(&c.maxWarnings, "max-warnings", c.maxWarnings, "warning limit (-1: none)")
	return fs
}

// parse accepts flags before, between and after positional arguments.
func parse(fs *flag.FlagSet, args []string) ([]string, error) {
	var pos []string
	for {
		if err := fs.Parse(args); err != nil {
			if errors.Is(err, flag.ErrHelp) {
				var b strings.Builder
				b.WriteString(cmdUsage[fs.Name()] + "\nFlags:\n")
				fs.SetOutput(&b)
				fs.PrintDefaults()
				fs.SetOutput(io.Discard)
				return nil, &helpError{b.String()}
			}
			return nil, usageErr("%v", err)
		}
		args = fs.Args()
		if len(args) == 0 {
			return pos, nil
		}
		pos = append(pos, args[0])
		args = args[1:]
	}
}

func (c *common) check() error {
	if c.format != "text" && c.format != "json" {
		return usageErr("--format must be text or json")
	}
	if c.maxDiag < 0 {
		return usageErr("--max-diagnostics must not be negative (0: no limit)")
	}
	return nil
}

func loadConfig(c *common) (*config.Config, error) {
	if c.configPath != "" {
		return config.Load(c.configPath, true)
	}
	return config.Load("docc.json", false)
}

// emit prints diagnostics and returns the exit code.
func emit(stdout, stderr io.Writer, c *common, p *engine.Project, ds []diag.Diagnostic) (int, error) {
	errs, warns := report.Counts(ds)
	limit, has := c.maxWarnings, c.maxWarnings >= 0
	failure := ""
	if !has && p.Cfg.HasMaxWarning {
		limit, has = p.Cfg.MaxWarnings, true
	}
	if has && warns > limit {
		failure = fmt.Sprintf("too many warnings: %d > --max-warnings %d", warns, limit)
	}
	if c.format == "json" {
		if err := report.JSON(stdout, ds, c.maxDiag, len(p.Docs), failure); err != nil {
			return 2, err
		}
	} else {
		if err := report.Text(stdout, ds, c.maxDiag, len(p.Docs)); err != nil {
			return 2, err
		}
		if failure != "" {
			say(stderr, "%s\n", failure)
		}
	}
	if errs > 0 || failure != "" {
		return 1, nil
	}
	return 0, nil
}

func cmdValidate(c *common, args []string, stdout, stderr io.Writer) (int, error) {
	fs := newFlags("validate", c)
	fix := fs.Bool("fix", false, "apply safe fixes")
	unsafe := fs.Bool("unsafe", false, "with --fix: also unsafe fixes")
	pos, err := parse(fs, args)
	if err != nil {
		return 2, err
	}
	if len(pos) > 0 {
		return 2, usageErr("unexpected argument %q", pos[0])
	}
	if err := c.check(); err != nil {
		return 2, err
	}
	if *unsafe && !*fix {
		return 2, usageErr("--unsafe needs --fix")
	}
	cfg, err := loadConfig(c)
	if err != nil {
		return 2, err
	}
	if *fix {
		edits, files, err := fixes.Run(func() (*engine.Project, error) { return engine.Load(cfg) }, *unsafe)
		if err != nil {
			return 2, err
		}
		if c.format == "text" && edits > 0 {
			say(stdout, "applied %d fixes in %d files\n", edits, files)
		}
	}
	p, err := engine.Load(cfg)
	if err != nil {
		return 2, err
	}
	return emit(stdout, stderr, c, p, p.Validate())
}

func cmdSchema(args []string, stdout io.Writer) error {
	fs := flag.NewFlagSet("schema", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	pos, err := parse(fs, args)
	if err != nil {
		return err
	}
	if len(pos) != 1 {
		return usageErr("usage: docc schema document|config")
	}
	switch pos[0] {
	case "document":
		_, err = stdout.Write(schema.Document())
	case "config":
		_, err = stdout.Write(schema.Config())
	default:
		return usageErr("unknown schema %q: use document or config", pos[0])
	}
	return err
}

// loadProject loads the config and the project and runs validation.
func loadProject(c *common) (*config.Config, *engine.Project, error) {
	cfg, err := loadConfig(c)
	if err != nil {
		return nil, nil, err
	}
	p, err := engine.Load(cfg)
	if err != nil {
		return nil, nil, err
	}
	p.Validate()
	return cfg, p, nil
}

func noArgs(pos []string) error {
	if len(pos) > 0 {
		return usageErr("unexpected argument %q", pos[0])
	}
	return nil
}

// build validates, and when there are no errors of groups 1xx-2xx writes the
// Markdown into out (or outFull) and returns the number of files. With single
// and html it also writes the one-file Markdown and HTML next to the directory
// and returns their paths in extra.
func build(p *engine.Project, full, single, html bool) (n int, extra []string, err error) {
	if engine.Structural(p.Diags) {
		return 0, nil, nil
	}
	dir := p.Cfg.Out
	if full {
		dir = p.Cfg.OutFull
	}
	if err := render.CheckDir(p, dir); err != nil {
		return 0, nil, err
	}
	r := &render.Renderer{P: p, Full: full}
	files := r.Files()
	one := map[string]string{} // extension -> content of a single-file build
	if single {
		one[".md"] = r.Single()
	}
	if html {
		one[".html"] = r.HTML()
	}
	paths := map[string]string{}
	for ext := range one {
		f, err := render.SingleFile(p, dir, ext)
		if err != nil {
			return 0, nil, err
		}
		paths[ext] = f
	}
	if err := render.Write(p.Cfg.Abs(dir), files); err != nil {
		return 0, nil, err
	}
	for _, ext := range []string{".md", ".html"} {
		if content, ok := one[ext]; ok {
			if err := render.WriteFile(paths[ext], content); err != nil {
				return 0, nil, err
			}
			extra = append(extra, dir+ext)
		}
	}
	return len(files), extra, nil
}

func cmdBuild(c *common, args []string, stdout, stderr io.Writer) (int, error) {
	fs := newFlags("build", c)
	full := fs.Bool("full", false, "audit build into outFull")
	single := fs.Bool("single", false, "also write all pages as one Markdown file next to the output directory")
	html := fs.Bool("html", false, "also write all pages as one self-contained HTML file next to the output directory")
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
	cfg, p, err := loadProject(c)
	if err != nil {
		return 2, err
	}
	n, extra, err := build(p, *full, *single, *html)
	if err != nil {
		return 2, err
	}
	code, err := emit(stdout, stderr, c, p, p.Sorted())
	if err != nil || c.format != "text" {
		return code, err
	}
	dir := cfg.Out
	if *full {
		dir = cfg.OutFull
	}
	if n > 0 {
		say(stdout, "built %d files in %s\n", n, dir)
		for _, f := range extra {
			say(stdout, "built %s\n", f)
		}
	} else if engine.Structural(p.Diags) {
		errs, _ := report.Counts(p.Diags)
		say(stdout, "build skipped: %d errors, %s not changed\n", errs, dir)
	}
	return code, err
}

func cmdCheck(c *common, args []string, stdout, stderr io.Writer) (int, error) {
	fs := newFlags("check", c)
	fs.Bool("fix", false, "")
	fs.Bool("unsafe", false, "")
	pos, err := parse(fs, args)
	if err == nil {
		err = noArgs(pos)
	}
	if err == nil {
		fs.Visit(func(f *flag.Flag) {
			if f.Name == "fix" || f.Name == "unsafe" {
				err = usageErr("--%s works only with validate", f.Name)
			}
		})
	}
	if err == nil {
		err = c.check()
	}
	if err != nil {
		return 2, err
	}
	cfg, p, err := loadProject(c)
	if err != nil {
		return 2, err
	}
	if !engine.Structural(p.Diags) {
		render.CheckFresh(p, cfg.Out, (&render.Renderer{P: p}).Files())
	}
	return emit(stdout, stderr, c, p, p.Sorted())
}

const initConfig = `{
  "src": "docs/src",
  "out": "docs/out",
  "roots": ["index"]
}
`

func cmdInit(c *common, args []string, stdout, stderr io.Writer) (int, error) {
	fs := newFlags("init", c)
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
	create := func(name, content string) error {
		if _, err := os.Stat(name); err == nil {
			say(stdout, "skipped %s: already exists\n", name)
			return nil
		}
		if err := os.MkdirAll(filepath.Dir(name), 0o755); err != nil {
			return err
		}
		if err := os.WriteFile(name, []byte(content), 0o644); err != nil {
			return err
		}
		say(stdout, "created %s\n", name)
		return nil
	}
	if err := create("docc.json", initConfig); err != nil {
		return 2, err
	}
	if err := create(filepath.Join("docs", "src", "index.json"), "{ \"title\": \"Index\" }\n"); err != nil {
		return 2, err
	}
	cfg, p, err := loadProject(c)
	if err != nil {
		return 2, err
	}
	n, _, err := build(p, false, false, false)
	if err != nil {
		return 2, err
	}
	if n > 0 {
		say(stdout, "built %d files in %s\n", n, cfg.Out)
	}
	code, err := emit(stdout, stderr, c, p, p.Sorted())
	if err == nil && c.format == "text" {
		say(stdout, "Next:\n  1. edit docs/src/index.json and add documents\n  2. docc validate --format json\n  3. agents: read skills/docc/SKILL.md in the docc repository\n")
	}
	return code, err
}
