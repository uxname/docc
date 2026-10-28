# docc

```text
       █
 ▄██████   ▄████▄   ▄████▄   ▄████▄
██    ██  ██    ██ ██       ██
██    ██  ██    ██ ██       ██
 ▀██████   ▀████▀   ▀████▀   ▀████▀
```

Compiler and linter for documentation written as JSON. AI agents write the documents, `docc` checks them like a code linter (structure, links, content, style) and builds Markdown for people, plus one HTML file to read and comment on, whatever the documentation is (technical, internal, for a customer, a book): a timeline of the saved versions that shows what changed in the text and the page as it was at any moment, and comments on selected words (typed or dictated) that come back as text for a messenger. The file holds nothing the reader should not see: no hidden notes, commit messages or authors.

## Principle

Documentation and code are written by AI agents; people only read them. People must not keep the links between documents and code in their heads: agents carry that routine, and `docc` keeps them from getting it wrong. Every link carries the version hash of what it points to. When a paragraph changes, every link to it breaks until an agent opens each place and checks it. When code changes, its fingerprint breaks until an agent checks it against the paragraphs. The build does not pass while any link is unchecked. There is no command that confirms everything at once: nothing can be confirmed without looking.

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
    docc validate    # check the sources (--format json for agents and CI; paths limit the report)
    docc build       # give codes to new blocks, write docs/out (--single, --html: also one .md file and one HTML app, kept up to date once made; --no-comments, --since; --page: one page; --rev: the HTML file of a Git revision)
    docc compile     # print pages, tree and diagnostics as one JSON object, write nothing (--rev: a Git revision, --hidden: with hidden documents)
    docc log         # the commits that changed the documents, with the files and block counts of each (JSON for programs)
    docc diff A B    # the blocks of the documents of two revisions lined up (--path: one document, every block)
    docc comments    # the readers' comments in "$comments" of the documents: list, apply (edits from stdin), import <file|->, rm <id>
    docc check       # for CI: validate + docs/out is up to date (unless commitOut is false); docc check backend reports only backend/
    docc codes       # give a code to every block that has none, nothing else (build does it too)
    docc migrate     # once: move a project of the old format (docc.lock, named anchors) to codes and hashes

A document is one JSON file in `docs/src`; its id is the file name (`docs/src/sign-in.json` is `sign-in`):

```json
{
  "title": "Sign-in",
  "kind": "page",
  "parent": "index",
  "body": ["After five wrong passwords sign-in locks for 15 minutes. ^k7f2"]
}
```

Pictures (`png`, `jpg`, `jpeg`, `gif`, `svg`, `webp`) lie inside `docs/src` and are linked by a path relative to the document: `![Plan](img/plan.svg)`. A fenced ```` ```mermaid ```` block is drawn as a diagram in the HTML file, offline. A form field (`{"$input": "radio", "id": "export", "label": "Export?", "options": ["Yes", "No"]}`) asks the readers of the HTML file; radio and checkbox fields also take an own answer.

Every document needs a `kind` and a schema `docs/schemas/<kind>.json` (rule DOC109). The project defines its own kinds; a schema sets `"additionalProperties"` to `true` (any extra field) or `false` (only the listed fields). Every text block ends with a permanent code, ` ^k7f2` (four signs of `23456789abcdefghjkmnpqrstuvwxyz`, unique in the project, shown to readers as `K7F2`). `docc build` gives one to a block written without it; nobody writes or changes codes by hand. Other documents link with `[[sign-in@9b1e]]`, to a field with `[[sign-in#field@77aa]]` or to one paragraph with `[[sign-in#^k7f2@3a9c]]`: the part after `@` is the version hash of the target. When the target changes, the link is an error (DOC209) whose message shows what changed (the old version is found in Git by its hash) and the new hash; an agent reads the change, checks the place and writes the new hash by hand. A project of the old format (a `docc.lock`, named anchors) is refused until `docc migrate` has moved it.

With `"trace": {"scan": ["src/**"]}` in `docc.json`, the tag `// docc: sign-in#^k7f2@3a9c ~e91d` links a code file to a paragraph; `~e91d` is the fingerprint of the file's code outside its regions, and a `// docc:begin sign-in#^k7f2@3a9c ~0c4f` … `// docc:end` region has its own. Any change of the code breaks its fingerprint (DOC402) until it is checked against the paragraphs and rewritten by hand.

In the HTML file a paragraph shows its code on hover with the buttons «Copy link» (`#sign-in--k7f2@3a9c`) and «Copy code».

A problem is reported with the position, the rule and a hint:

    docs/src/sign-in.json:4:13: error graph/broken-ref (DOC202) parent points to a document that does not exist: "login"
      hint: Fix the id, or create docs/src/login.json only if the document is really needed.
    checked 2 files: 2 errors, 0 warnings

Two fields are for agents and auditors, not for readers: `notes` (doubts and open points, read before editing) and `"hidden": true` (checked, but no page). `docc build --full` writes an audit build with both into `docs/out-full` (not committed). A free field spelled almost like a reserved one (`parnet`) is warned about (DOC110).

`docc validate --format json` prints `{"summary": {...}, "diagnostics": [...]}`; `docc explain graph/broken-ref` (or its code DOC202) describes a rule; `docc <command> --help` lists the flags.

The documentation of the project is in `docs/src` (JSON) and `docs/out` (generated Markdown, start at `docs/out/index.md`). The full description is in `docc_spec.md`.

## License

MIT, see `LICENSE`.
