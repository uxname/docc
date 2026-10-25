# docc

```text
       █
 ▄██████   ▄████▄   ▄████▄   ▄████▄
██    ██  ██    ██ ██       ██
██    ██  ██    ██ ██       ██
 ▀██████   ▀████▀   ▀████▀   ▀████▀
```

Compiler and linter for documentation written as JSON. AI agents write the documents, `docc` checks them like a code linter (structure, links, content, style) and builds Markdown for people, plus one HTML file to read and comment on, whatever the documentation is (technical, internal, for a customer, a book): a timeline of the saved versions that shows what changed in the text and the page as it was at any moment, and comments on selected words (typed or dictated) that come back as text for a messenger. The file holds nothing the reader should not see: no hidden notes, commit messages or authors.

## Install

Linux and macOS:

    curl -fsSL https://raw.githubusercontent.com/uxname/docc/main/install.sh | sh

Windows (PowerShell):

    irm https://raw.githubusercontent.com/uxname/docc/main/install.ps1 | iex

This downloads the release binary for your system, checks its SHA-256 and puts `docc` into `~/.local/bin` (Windows: `%LOCALAPPDATA%\docc\bin`). Set `DOCC_INSTALL_DIR` to change the folder and `DOCC_VERSION=v0.1.0` to pin a version.

With stable Rust you can build it yourself: `cargo install --git https://github.com/uxname/docc`, or clone the repository and run `cargo build --release` (the binary is `target/release/docc`). Such a build reports version `dev`.

## Update

    docc update            # latest release; docc update --check only looks, --version v0.1.0 pins

Running the install command again does the same. A build from source reports `dev`, so `docc update` refuses it: rebuild it with `cargo install --git https://github.com/uxname/docc --force` or switch to the install command.

## Agent skill

The skill teaches an AI agent (Claude Code, Codex, Cursor and others) to write documents and read diagnostics:

    npx skills add uxname/docc

Update it with `npx skills update`. The skill needs the `docc` binary from above.

## Quick start

    docc init        # docc.json (maxWarnings 0, commitOut false), docs/schemas/page.json, docs/src/index.json, .gitignore lines, docs/out, a git pre-commit hook
    docc validate    # check the sources (--format json for agents and CI)
    docc build       # write docs/out and docc.lock (--single, --html: also one .md file and one HTML app, kept up to date once made; --no-comments, --since; --page: one page; --rev: the HTML file of a Git revision)
    docc compile     # print pages, tree and diagnostics as one JSON object, write nothing (--rev: a Git revision, --hidden: with hidden documents)
    docc log         # the commits that changed the documents, with the files and block counts of each (JSON for programs)
    docc diff A B    # the blocks of the documents of two revisions lined up (--path: one document, every block)
    docc comments    # the readers' comments in "$comments" of the documents: list, apply (edits from stdin), import <file|->, rm <id>
    docc check       # for CI: validate + docs/out (unless commitOut is false) and docc.lock are up to date
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

Pictures (`png`, `jpg`, `jpeg`, `gif`, `svg`, `webp`) lie inside `docs/src` and are linked by a path relative to the document: `![Plan](img/plan.svg)`. A fenced ```` ```mermaid ```` block is drawn as a diagram in the HTML file, offline. A form field (`{"$input": "radio", "id": "export", "label": "Export?", "options": ["Yes", "No"]}`) asks the readers of the HTML file; radio and checkbox fields also take an own answer.

Every document needs a `kind` and a schema `docs/schemas/<kind>.json` (rule DOC109). The project defines its own kinds; a schema sets `"additionalProperties"` to `true` (any extra field) or `false` (only the listed fields). Other documents link with `[[sign-in]]` or to one paragraph with `[[sign-in#^lockout]]`.

`docc build` records every link with a hash of its target in `docc.lock` (committed). When the target changes, the source gets a warning (DOC209) until someone checks it and runs `docc ack`; `ack` confirms only the links that changed and lists them, so read every source it lists. A stale link fails `docc check` in a project made by `docc init`.

With `"trace": {"scan": ["src/**"]}` in `docc.json`, a comment `// docc: sign-in#^lockout` links code to a document; a `// docc:begin` … `// docc:end` region warns (DOC402) when the code changes.

A problem is reported with the position, the rule and a hint:

    docs/src/sign-in.json:4:13: error graph/broken-ref (DOC202) parent points to a document that does not exist: "login"
      hint: Fix the id, or create docs/src/login.json only if the document is really needed.
    checked 2 files: 2 errors, 0 warnings

Two fields are for agents and auditors, not for readers: `notes` (doubts and open points, read before editing) and `"hidden": true` (checked, but no page). `docc build --full` writes an audit build with both into `docs/out-full` (not committed). A free field spelled almost like a reserved one (`parnet`) is warned about (DOC110).

`docc validate --format json` prints `{"summary": {...}, "diagnostics": [...]}`; `docc explain graph/broken-ref` (or its code DOC202) describes a rule; `docc <command> --help` lists the flags.

The documentation of the project is in `docs/src` (JSON) and `docs/out` (generated Markdown, start at `docs/out/index.md`). The full description is in `docc_spec.md`.

## License

MIT, see `LICENSE`.
