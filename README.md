# docc

```text
       █
 ▄██████   ▄████▄   ▄████▄   ▄████▄
██    ██  ██    ██ ██       ██
██    ██  ██    ██ ██       ██
 ▀██████   ▀████▀   ▀████▀   ▀████▀
```

Compiler and linter for documentation written as JSON. AI agents write the documents, `docc` checks them like a code linter (structure, links, content, style) and builds Markdown for people, plus one HTML file to read and review: the Git history of every paragraph, comments on blocks (typed or dictated) and Markdown export.

## Install

Linux and macOS:

    curl -fsSL https://raw.githubusercontent.com/uxname/docc/main/install.sh | sh

Windows (PowerShell):

    irm https://raw.githubusercontent.com/uxname/docc/main/install.ps1 | iex

This downloads the release binary for your system, checks its SHA-256 and puts `docc` into `~/.local/bin` (Windows: `%LOCALAPPDATA%\docc\bin`). Set `DOCC_INSTALL_DIR` to change the folder and `DOCC_VERSION=v0.1.0` to pin a version.

With Go 1.27.1 or newer you can build it yourself: `go install github.com/uxname/docc/cmd/docc@latest`, or clone the repository and run `go build -o docc ./cmd/docc`.

## Update

    docc update            # latest release; docc update --check only looks, --version v0.1.0 pins

Running the install command again does the same. A `go install` build is updated with `go install ...@latest`.

## Agent skill

The skill teaches an AI agent (Claude Code, Codex, Cursor and others) to write documents and read diagnostics:

    npx skills add uxname/docc

Update it with `npx skills update`. The skill needs the `docc` binary from above.

## Quick start

    docc init        # docc.json (maxWarnings 0), docs/schemas/page.json, docs/src/index.json, .gitignore lines, docs/out, a git pre-commit hook
    docc validate    # check the sources (--format json for agents and CI)
    docc build       # write docs/out and docc.lock (--single, --html: also one .md file and one HTML app)
    docc comments    # review comments from the HTML file: import <file>, list, done <id>, rm <id>
    docc check       # for CI: validate + docs/out and docc.lock are up to date
    docc ack <id>    # after a review: confirm the changed links around a document or code file (prints each one)

A document is one JSON file in `docs/src`; its id is the file name (`docs/src/sign-in.json` is `sign-in`):

```json
{
  "title": "Sign-in",
  "kind": "page",
  "parent": "index",
  "body": ["After five wrong passwords sign-in locks for 15 minutes. ^lockout"]
}
```

Every document needs a `kind` and a schema `docs/schemas/<kind>.json` (rule DOC109). The project defines its own kinds; a schema sets `"additionalProperties"` to `true` (any extra field) or `false` (only the listed fields). Other documents link with `[[sign-in]]` or to one paragraph with `[[sign-in#^lockout]]`.

`docc build` records every link with a hash of its target in `docc.lock` (committed). When the target changes, the source gets a warning (DOC209) until someone checks it and runs `docc ack`; `ack` confirms only the links that changed and lists them, so read every source it lists. A stale link fails `docc check` in a project made by `docc init`.

With `"trace": {"scan": ["src/**"]}` in `docc.json`, a comment `// docc: sign-in#^lockout` links code to a document; a `// docc:begin` … `// docc:end` region warns (DOC402) when the code changes.

A problem is reported with the position, the rule and a hint:

    docs/src/sign-in.json:4:13: error DOC202 (graph/broken-ref) parent points to a document that does not exist: "login"
      hint: Fix the id, or create docs/src/login.json only if the document is really needed.
    checked 2 files: 2 errors, 0 warnings

Two fields are for agents and auditors, not for readers: `notes` (doubts and open points, read before editing) and `"hidden": true` (checked, but no page). `docc build --full` writes an audit build with both into `docs/out-full` (not committed). A free field spelled almost like a reserved one (`parnet`) is warned about (DOC110).

`docc validate --format json` prints `{"summary": {...}, "diagnostics": [...]}`; `docc explain DOC202` describes a rule; `docc <command> --help` lists the flags.

The documentation of the project is in `docs/src` (JSON) and `docs/out` (generated Markdown, start at `docs/out/index.md`). The full description is in `docc_spec.md`.

## License

MIT, see `LICENSE`.
