# docc

Compiler and linter for documentation written as JSON. AI agents write the documents, `docc` checks them like a code linter (structure, links, content, style) and builds Markdown for people.

Install: download a release archive (binary and `skills/docc/SKILL.md`), or `go install github.com/santiment/docc/cmd/docc@latest`.

    docc init        # docc.json, docs/schemas/page.json, docs/src/index.json, docs/out, a git pre-commit hook
    docc validate    # check the sources (--format json for agents and CI)
    docc build       # write docs/out and docc.lock (--single, --html: also one .md and one .html file for reading)
    docc check       # for CI: validate + docs/out and docc.lock are up to date
    docc ack <id>    # confirm the links around a changed document or code file after a review

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

`docc build` records every link with a hash of its target in `docc.lock` (committed). When the target changes, the source gets a warning (DOC209) until someone checks it and runs `docc ack`.

With `"trace": {"scan": ["src/**"]}` in `docc.json`, a comment `// docc: sign-in#^lockout` links code to a document; a `// docc:begin` … `// docc:end` region warns (DOC402) when the code changes.

A problem is reported with the position, the rule and a hint:

    docs/src/sign-in.json:4:13: error DOC202 (graph/broken-ref) parent points to a document that does not exist: "login"
      hint: Create docs/src/login.json or fix the id.
    checked 2 files: 2 errors, 0 warnings

`docc validate --format json` prints `{"summary": {...}, "diagnostics": [...]}`; `docc explain DOC202` describes a rule; `docc <command> --help` lists the flags.

The documentation of the project is in `docs/src` (JSON) and `docs/out` (generated Markdown, start at `docs/out/index.md`). Agents: copy `skills/docc/SKILL.md` into your project.
