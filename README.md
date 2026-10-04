# docc

Compiler and linter for documentation written as JSON. AI agents write the documents, `docc` checks them like a code linter (structure, links, content, style) and builds Markdown for people.

Install: download a release archive (binary and `skills/docc/SKILL.md`), or `go install github.com/santiment/docc/cmd/docc@latest`.

    docc init        # docc.json, docs/src/index.json, docs/out
    docc validate    # check the sources
    docc build       # write docs/out
    docc check       # for CI: validate + docs/out is up to date

The documentation of the project is in `docs/src` (JSON) and `docs/out` (generated Markdown, start at `docs/out/index.md`). Agents: copy `skills/docc/SKILL.md` into your project.
