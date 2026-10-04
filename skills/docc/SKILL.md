---
name: docc
description: Write, edit and fix project documentation (requirements, decisions, goals, questions) stored as JSON in docs/src and checked by the docc linter. Use whenever a task touches docs/src or docc.json, or asks to document, record a requirement or decision, or update project documentation in a project that has a docc.json, even if another document skill also fits. Covers docc validate, docc build and reading diagnostics.
---

# docc

`docc` checks documentation written as JSON files (structure, links, text, style) and builds readable Markdown from it. The JSON in `docs/src` is the source of truth. `docs/out` is generated: never edit it by hand.

## The loop

After every change in `docs/src`, run both commands in this order:

1. `docc validate --format json` — read the diagnostics, fix the sources, repeat until `summary.errors` is 0.
2. `docc build` — writes `docs/out`. Commit sources and output together.

CI runs `docc check`: it fails on errors and when `docs/out` differs from the sources. If it reports DOC501, run `docc build`.

Exit codes: `0` clean, `1` errors (or too many warnings), `2` bad usage or bad `docc.json` / schema files (the message goes to stderr).

## Document format

One document is one `*.json` file in `docs/src` (any folder depth). **The id is the file name without `.json`**: lowercase `a-z`, `0-9`, `-`, not starting or ending with `-`. Ids must be unique across folders. Run `docc schema document` for the exact schema of the fields; run `docc schema config` for `docc.json`.

```json
{
  "title": "Lock the account after five wrong passwords",
  "kind": "requirement",
  "status": "accepted",
  "parent": "goal-secure-login",
  "links": ["adr-lockout-policy"],
  "body": ["First paragraph.", "Second paragraph."],
  "statement": "After five wrong passwords in a row the system locks sign-in for 15 minutes. See [[adr-lockout-policy]]."
}
```

- `title` is required. `kind`, `status`, `parent` (one id), `links` (ids), `tags`, `body`, `notes`, `hidden`, `reviewed`, `docIgnore` are the reserved fields. Any other field is free and holds prose.
- Write long text as an array of paragraphs, one paragraph per line of the file: `"body": ["One.", "Two."]`. Inside a paragraph a line break is `\n`.
- Link to another document inside text with `[[id]]`, or to one of its fields with `[[id#field]]`. The id must exist. Put every document you link in the text also into `links` (or `parent`); `docc validate --fix` can add the missing ones.
- Every visible document must be reachable from a root (default root: the document `index`) through `parent`, `links` or `[[id]]`. Reachability checks only the shape of the graph. Whether a link makes sense is for a human reviewer, so link a document where a reader would look for it, not only to silence the error.
- A kind schema `docs/schemas/<kind>.json` (JSON Schema 2020-12) checks every document of that kind. You may write such schemas yourself. A schema can `$ref` other files inside the schemas folder and nothing else (no remote or outside references).

## Hidden material

- `notes`: an array of strings for agents, never printed to Markdown. Read the notes of a document before you edit it. Links `[[id]]` in notes are checked but do not make a document reachable.
- `"hidden": true`: the document is checked but gets no page in `docs/out`, and is not required to be reachable. A visible document cannot link to a hidden one (DOC210).
- `{{id#field}}` inside `body` or a free field is replaced at build time by the text of that field of another document, so a fact is stored once. Write the whole paragraph as `"{{id#field}}"` to insert all paragraphs of the target. Not allowed: `{{id}}` without a field, service fields, cycles.
- `reviewed`: `{"target-id": "8-hex-fingerprint"}` confirms a link after you checked the target. Rule DOC209 (off by default) prints the expected fingerprint in its hint. Never write a fingerprint without actually reading the target.
- `docc build --full` writes everything into `docs/out-full` (not committed): hidden documents, notes, suppressions, reviewed entries and include boundaries. Use it to audit what agents work with.

## Reading diagnostics

`docc validate --format json` prints an envelope: `{"summary": {checked, errors, warnings, shown, truncated}, "diagnostics": [{code, rule, level, file, line, column, endLine, endColumn, message, hint, fixable}]}`. Positions point into the JSON source. Each `hint` says how to fix the problem. The text format prints one line per finding: `file:line:col: level CODE (group/name) message`.

- By default only the first 20 diagnostics are shown. If `summary.truncated` is true, fix those and run again, or run `docc validate --format json --max-diagnostics 0` for all of them.
- `docc explain DOC301` or `docc explain style/hedging` describes one rule with a violating and a fixed example (a code or a name, not a group). `docc rules` lists every rule with its level.
- `fixable: "safe"` is applied by `docc validate --fix`; `"unsafe"` needs `--fix --unsafe`. Review the diff afterwards.
- Warnings do not fail the run unless `--max-warnings` or `maxWarnings` is set. Vague words (`style/hedging`, DOC352) and time-bound words (`style/timeless-words`, DOC353) are warnings: fix them when the cause is real.
- `docc <command> --help` prints the flags of a command.
- DOC901 means a `docIgnore` entry suppresses nothing: delete it.

## Starting a project

`docc init` creates `docc.json` and `docs/src/index.json` (the root). Then declare the vocabulary in `docc.json`, otherwise a typo in `kind` or a tag is not caught:

```json
{ "kinds": ["goal", "requirement", "decision", "question"], "tags": ["security", "ux"] }
```

Suggested kinds: `goal`, `requirement`, `decision`, `question`. Suggested statuses: `draft`, `accepted`, `superseded` (a question: `open`, `closed`). To enforce fields and statuses write `docs/schemas/<kind>.json`. One fact is one document: do not put several requirements into one file; each gets its own file with its own `parent`.

## Changing a decision

Write a new decision document. Set the old one to `"status": "superseded"` and link it to the new one. `docc` does not compare numbers in free fields, so after the change search for the old value yourself: `grep -rn "old value" docs/src code config`, then fix every hit. Do not rewrite history by editing the old decision's text.

## Code tags

With `"trace": {"scan": ["code/**"]}` in `docc.json`, a comment `// @covers req-lockout, req-password-length` in code links the file to documents. Ids are separated by commas; with a space only the first id is read (DOC401 reports it). docc checks that each id exists, nothing more. The pages in `docs/out` show `Covered by` and `Referenced by`.

## notes or hidden

`notes` holds doubts and open points inside a visible document. `"hidden": true` is for a document that must not be published. Do not duplicate one fact into both. Unknown or vague input goes to `notes`, or to a document with `"kind": "question"`, `"status": "open"`.

## Rules for you

1. Fix the cause. Do **not** add `docIgnore` to silence a diagnostic you could fix. Use it only for a real exception and always give a `reason` that a reviewer can accept:
   `"docIgnore": [{ "rule": "DOC301", "reason": "term from the customer's spec", "path": "/statement" }]`.
   Core rules (syntax, graph, build) cannot be suppressed.
2. Link only to ids that exist. Never invent an id to remove DOC202; create the document or remove the link.
3. Do not edit `docs/out` or `docs/out-full`. Edit `docs/src`, then `docc validate` and `docc build`.
4. Do not rename a document without updating every `parent`, `links` and `[[id]]` that points to it.
5. Keep facts in one place: use `{{id#field}}` or a link instead of copying text.
6. State facts. Replace words such as "fast", "easy", "soon" with measurable statements; leave no TODO or ??? in committed text.
7. Do **not** invent numbers, dates, reasons or decisions that the source does not contain. If the source says "a few weeks", do not write "4 weeks": put the doubt into `notes` or create a question document (`"kind": "question"`, `"status": "open"`).
8. One fact, one document. Never pack several requirements into one file.
9. A literal `[[id]]`, `{{ }}` or `<tag>` in prose goes into backticks; otherwise docc reads it as a link, an include or HTML.

## Before you finish

1. `docc validate --format json`: `summary.errors` is 0, and every warning is fixed or justified.
2. `docc build`, then `docc check` passes.
3. No invented facts: every number and reason comes from the source.
4. Every document you linked is in `links` or `parent`; no document is left unreachable.
5. After changing a decision, the old values are gone from `docs/src`, code and config.

## Hook for Claude Code

Run the loop automatically and forbid edits to the generated folder. In `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "Edit|Write|MultiEdit", "hooks": [{ "type": "command", "command": ".claude/hooks/docc-guard.sh" }] }
    ],
    "PostToolUse": [
      { "matcher": "Edit|Write|MultiEdit", "hooks": [{ "type": "command", "command": ".claude/hooks/docc-after-edit.sh" }] }
    ]
  }
}
```

`.claude/hooks/docc-guard.sh` blocks writes into generated output (exit code 2 sends the message back to the model):

```sh
#!/bin/sh
f=$(jq -r '.tool_input.file_path // empty')
case "$f" in
  */docs/out/*|*/docs/out-full/*) echo "docs/out is generated by docc: edit docs/src and run docc build" >&2; exit 2 ;;
esac
```

`.claude/hooks/docc-after-edit.sh` validates and builds after a source edit. Errors block (stderr and exit code 2 return to the model); warnings are printed to stderr and sent to the model as additional context without blocking:

```sh
#!/bin/sh
f=$(jq -r '.tool_input.file_path // empty')
case "$f" in
  */docs/src/*|*/docs/schemas/*|*/docc.json) ;;
  *) exit 0 ;;
esac
out=$(docc validate --format json --max-diagnostics 0); code=$?
list=$(echo "$out" | jq -r '(.diagnostics // [.])[] | "\(.file):\(.line):\(.column): \(.level // "error") \(.code) \(.message) Hint: \(.hint)"' 2>/dev/null) || list=$out
if [ "$code" -ne 0 ]; then echo "$list" >&2; exit 2; fi
if [ -n "$list" ]; then
  echo "$list" >&2
  jq -n --arg c "$list" '{hookSpecificOutput:{hookEventName:"PostToolUse",additionalContext:$c}}'
fi
docc build >/dev/null || exit 2
```

Make both scripts executable. The way diagnostics return to the model depends on the tool: for Claude Code it is stderr plus exit code 2 from the hook; other agents need their own equivalent.

## CI step

```yaml
- name: Check documentation
  run: docc check
```

Install the binary from the release archive, or with `go install github.com/santiment/docc/cmd/docc@latest`. Mark generated files in `.gitattributes` so reviews collapse them: `docs/out/** linguist-generated`.
