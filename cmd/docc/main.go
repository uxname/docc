// Command docc is a compiler and linter for JSON documentation.
package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/santiment/docc/internal/config"
	"github.com/santiment/docc/internal/diag"
	"github.com/santiment/docc/internal/engine"
	"github.com/santiment/docc/internal/report"
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

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

// usageError makes run exit with code 2 and a message on stderr.
type usageError struct{ msg string }

func (e *usageError) Error() string { return e.msg }

func usageErr(format string, a ...any) error { return &usageError{fmt.Sprintf(format, a...)} }

func run(args []string, stdout, stderr io.Writer) int {
	if len(args) == 0 {
		fmt.Fprint(stderr, usage)
		return 2
	}
	var err error
	code := 0
	switch args[0] {
	case "--version", "-version", "version":
		fmt.Fprintf(stdout, "docc %s\n", version)
		return 0
	case "--help", "-h", "help":
		fmt.Fprint(stdout, usage)
		return 0
	case "validate":
		code, err = cmdValidate(args[1:], stdout)
	default:
		err = usageErr("unknown command %q (try docc --help)", args[0])
	}
	if err != nil {
		var ue *usageError
		var ce *config.Error
		var eu *engine.UsageError
		switch {
		case errors.As(err, &ue), errors.As(err, &eu):
			fmt.Fprintf(stderr, "docc: error: %s\n", err)
		case errors.As(err, &ce):
			fmt.Fprintln(stderr, ce.Error())
		default:
			fmt.Fprintf(stderr, "docc: error: %v\n", err)
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
	} else {
		report.Text(stdout, ds, c.maxDiag)
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
	p, err := engine.Load(cfg)
	if err != nil {
		return 2, err
	}
	ds := p.Validate()
	return emit(stdout, &c, cfg, ds)
}
