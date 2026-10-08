# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

docc is a Go CLI (single binary, module `github.com/uxname/docc`, Go 1.27.1) that lints documentation written as JSON in `docs/src` and builds Markdown/HTML from it. The authoritative description is `docc_spec.md`; the project documents itself in `docs/src` (JSON) with generated output in `docs/out`.

## Commands

```sh
go build -o docc ./cmd/docc
go test ./...                                   # everything
go test ./cmd/docc -run TestName                # one test
go test ./cmd/docc -run Golden -update          # rewrite testdata/*/expected.json after an intended change
DOCC_SCALE=1 go test ./cmd/docc -run Scale -v   # performance check on 1000 documents (skipped by default)
docc build && docc check --max-warnings 0       # the repo's own docs must stay clean
scripts/release.sh v0.1.0                       # 5 archives + checksums.txt into dist/ (gitignored)
```

After changing anything in `docs/src` run `docc build` (updates `docs/out` and `docc.lock`). Editing a document that others link to raises DOC209 on the linking documents: read them, then `docc ack <id>`.

## Architecture

Pipeline: `loader` (strict JSON with offsets) → `schema` (built-in document schema + per-kind schemas from `docs/schemas`) → `engine` (reference graph, rules, suppressions, `docc.lock`) → `render` (Markdown folder, single `.md`, single `.html`) → freshness comparison for `check`. `cmd/docc` is only flag parsing and dispatch (`main.go` `dispatch`, one `cmdX` per command).

- **Rules** live in `internal/rules/registry.go`; `explain`, `rules`, the skill and docs are derived from it. A new rule needs a registry entry, an `engine` check, a `testdata/<CODE>/` fixture and an `expected.json`. `TestSkillMatchesRegistryAndCommands` fails if the skill mentions an unknown `DOCxxx` code or `docc <command>`; add new commands to its list in `cmd/docc/coverage_test.go`.
- **Rendering**: HTML is built from the same Markdown via goldmark; `page.html/css/js` are embedded. Page labels are written as `{{t:...}}` and need a Russian entry in `internal/render/lang.go` (a test fails on a missing or unused one). The Markdown output is never branded.
- **Text matching** (custom rules, DOC313) is done in NFC with positions mapped back to the source.
- **`docc.lock`** stores every link with a hash of its target; this is what DOC209/DOC402/DOC503 are computed from (`internal/engine/lock.go`).
- Tests are mostly golden: `testdata/<case>/` is a project plus `expected.json`; `cmd/docc/*_test.go` build throwaway projects with a `project(t, map[path]content)` helper and run the CLI via `do(...)`.

## Release and self-update

- `install.sh` / `install.ps1` and `docc update` (`cmd/docc/update.go`) all resolve versions from `checksums.txt` of a GitHub Release (`latest/download/checksums.txt`, `download/<tag>/...`); the archive name `docc_<tag>_<os>_<arch>.<tar.gz|zip>` is the contract between `scripts/release.sh`, the installers and `archiveRe` in `update.go`. `DOCC_BASE_URL` points all three at a mirror for testing.
- `.github/workflows/release.yml` runs on tags `v*`. The version is injected with `-X main.version`; a plain `go build` reports `dev` and `docc update` refuses it.
- The skill `skills/docc/SKILL.md` is installed by users with `npx skills add uxname/docc`; keep its frontmatter `name`/`description`. `skills/docc/evals/` holds fixture projects (its `code/` folders are Go packages picked up by `./...`) and `evals.md` explains how to run them.

## Working rules for this repo

- After any change: run `go test ./...`, `docc build`, `docc check`, commit on `main`, and rebuild the installed binary with `go build -o ~/.local/bin/docc ./cmd/docc`. Before replacing it, run the new build on `~/Документы/Work/o2` (`docc check --max-warnings 0`) because that project's hooks call `docc` from PATH.
- Skill and project documents are written in English; user-facing talk is in Russian.
- Do not add CI beyond the two workflows without being asked; do not push or tag without being asked.
