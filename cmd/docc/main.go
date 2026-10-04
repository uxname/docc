// Command docc is a compiler and linter for JSON documentation.
package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"

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

Usage: docc <command> [flags]

Commands:
  init               create docc.json and docs/src/index.json, build docs/out
  validate           check sources (--fix applies safe fixes)
  build              build Markdown into docs/out (--full: audit build)
  check              read-only check for CI: validate + freshness of out/
  explain <code>     describe a rule
  rules              list rules with effective levels
  schema document|config   print a JSON Schema

Flags: --format text|json, --config <file>, --max-diagnostics N, --max-warnings N
`

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
	var err error
	code := 0
	switch args[0] {
	case "--version", "-version", "version":
		say(stdout, "docc %s\n", version)
		return 0
	case "--help", "-h", "help":
		say(stdout, "%s", usage)
		return 0
	case "validate":
		code, err = cmdValidate(args[1:], stdout)
	case "build":
		code, err = cmdBuild(args[1:], stdout)
	case "check":
		code, err = cmdCheck(args[1:], stdout)
	case "init":
		code, err = cmdInit(args[1:], stdout)
	case "schema":
		err = cmdSchema(args[1:], stdout)
	default:
		err = usageErr("unknown command %q (try docc --help)", args[0])
	}
	if err != nil {
		var ue *usageError
		var ce *config.Error
		var eu *engine.UsageError
		switch {
		case errors.As(err, &ue), errors.As(err, &eu):
			say(stderr, "docc: error: %s\n", err)
		case errors.As(err, &ce):
			say(stderr, "%s\n", ce.Error())
		default:
			say(stderr, "docc: error: %v\n", err)
		}
		return 2
	}
	return code
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
	fs.StringVar(&c.format, "format", "text", "text or json")
	fs.StringVar(&c.configPath, "config", "", "path to docc.json")
	fs.IntVar(&c.maxDiag, "max-diagnostics", 20, "text output limit (0: no limit)")
	fs.IntVar(&c.maxWarnings, "max-warnings", -1, "warning limit")
	return fs
}

// parse accepts flags before, between and after positional arguments.
func parse(fs *flag.FlagSet, args []string) ([]string, error) {
	var pos []string
	for {
		if err := fs.Parse(args); err != nil {
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
	return nil
}

func loadConfig(c *common) (*config.Config, error) {
	if c.configPath != "" {
		return config.Load(c.configPath, true)
	}
	return config.Load("docc.json", false)
}

// emit prints diagnostics and returns the exit code.
func emit(stdout io.Writer, c *common, cfg *config.Config, ds []diag.Diagnostic) (int, error) {
	if c.format == "json" {
		if err := report.JSON(stdout, ds); err != nil {
			return 2, err
		}
	} else if err := report.Text(stdout, ds, c.maxDiag); err != nil {
		return 2, err
	}
	errs, warns, _ := report.Counts(ds)
	limit, has := c.maxWarnings, c.maxWarnings >= 0
	if !has && cfg.HasMaxWarning {
		limit, has = cfg.MaxWarnings, true
	}
	if errs > 0 || (has && warns > limit) {
		return 1, nil
	}
	return 0, nil
}

func cmdValidate(args []string, stdout io.Writer) (int, error) {
	var c common
	fs := newFlags("validate", &c)
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
	cfg, err := loadConfig(&c)
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
	return emit(stdout, &c, cfg, p.Validate())
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
// Markdown into out (or outFull) and returns the number of files.
func build(p *engine.Project, full bool) (int, error) {
	if engine.Structural(p.Diags) {
		return 0, nil
	}
	dir := p.Cfg.Out
	if full {
		dir = p.Cfg.OutFull
	}
	if err := render.CheckDir(p, dir); err != nil {
		return 0, err
	}
	files := (&render.Renderer{P: p, Full: full}).Files()
	if err := render.Write(p.Cfg.Abs(dir), files); err != nil {
		return 0, err
	}
	return len(files), nil
}

func cmdBuild(args []string, stdout io.Writer) (int, error) {
	var c common
	fs := newFlags("build", &c)
	full := fs.Bool("full", false, "audit build into outFull")
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
	cfg, p, err := loadProject(&c)
	if err != nil {
		return 2, err
	}
	n, err := build(p, *full)
	if err != nil {
		return 2, err
	}
	code, err := emit(stdout, &c, cfg, p.Sorted())
	if err == nil && n > 0 && c.format == "text" {
		dir := cfg.Out
		if *full {
			dir = cfg.OutFull
		}
		say(stdout, "built %d files in %s\n", n, dir)
	}
	return code, err
}

func cmdCheck(args []string, stdout io.Writer) (int, error) {
	var c common
	fs := newFlags("check", &c)
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
	cfg, p, err := loadProject(&c)
	if err != nil {
		return 2, err
	}
	if !engine.Structural(p.Diags) {
		render.CheckFresh(p, cfg.Out, (&render.Renderer{P: p}).Files())
	}
	return emit(stdout, &c, cfg, p.Sorted())
}

const initConfig = `{
  "src": "docs/src",
  "out": "docs/out",
  "roots": ["index"]
}
`

func cmdInit(args []string, stdout io.Writer) (int, error) {
	var c common
	fs := newFlags("init", &c)
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
	cfg, p, err := loadProject(&c)
	if err != nil {
		return 2, err
	}
	n, err := build(p, false)
	if err != nil {
		return 2, err
	}
	if n > 0 {
		say(stdout, "built %d files in %s\n", n, cfg.Out)
	}
	return emit(stdout, &c, cfg, p.Sorted())
}
