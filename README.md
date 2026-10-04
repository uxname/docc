# docc

Compiler and linter for documentation written as JSON. AI agents write the documents, `docc` checks them like a code linter (structure, links, content, style) and builds Markdown for people.

Install: download a release archive (binary and `skills/docc/SKILL.md`), or `go install github.com/santiment/docc/cmd/docc@latest`.

    docc init        # docc.json, docs/src/index.json, docs/out
    docc validate    # check the sources (--format json for agents and CI)
    docc build       # write docs/out
    docc check       # for CI: validate + docs/out is up to date

A document is one JSON file in `docs/src`; its id is the file name (`docs/src/req-lockout.json` is `req-lockout`):

```json
{
  "title": "Lock the account after five wrong passwords",
  "kind": "requirement",
  "parent": "index",
  "statement": "After five wrong passwords the system locks sign-in for 15 minutes."
}
```

A problem is reported with the position, the rule and a hint:

    docs/src/req-lockout.json:4:13: error DOC202 (graph/broken-ref) parent points to a document that does not exist: "goal-login"
      hint: Create docs/src/goal-login.json or fix the id.
    checked 2 files: 2 errors, 0 warnings

`docc validate --format json` prints `{"summary": {...}, "diagnostics": [...]}`; `docc explain DOC202` describes a rule; `docc <command> --help` lists the flags.

The documentation of the project is in `docs/src` (JSON) and `docs/out` (generated Markdown, start at `docs/out/index.md`). Agents: copy `skills/docc/SKILL.md` into your project.
