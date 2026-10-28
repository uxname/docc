# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

docc is a Rust CLI (single binary, crate `docc` in the repository root, stable Rust, edition 2024) that lints documentation written as JSON in `docs/src` and builds Markdown/HTML from it. The authoritative description is `docc_spec.md`; the project documents itself in `docs/src` (JSON) with generated output in `docs/out`.

## Commands

```sh
cargo build --release                                     # target/release/docc
cargo test                                                # everything (unit tests in src/, CLI tests in tests/)
cargo test --test cli name                                # one test of tests/cli.rs
DOCC_UPDATE=1 cargo test --test golden                    # rewrite testdata/*/expected.json after an intended change
DOCC_SCALE=1 cargo test --release --test scale -- --nocapture  # performance check on 1000 documents (skipped by default)
docc build && docc check --max-warnings 0                 # the repo's own docs must stay clean
scripts/release.sh v0.1.0                                 # 5 archives + checksums.txt into dist/ (gitignored); needs cargo-zigbuild and zig
scripts/difftest.sh OLD_BIN NEW_BIN                       # compare two builds on testdata, evals, this repo and o2
```

After changing anything in `docs/src` run `docc build` (gives codes to new blocks, updates `docs/out`). Editing a document that others link to raises DOC209 on the linking documents: read each of them, then write the new hash into that link by hand. Never touch a block code ` ^PAY-14` or a document's `key`.

**Principle.** Documentation and code are written by AI agents; people only read them. People must not keep the links between documents and code in their heads: agents carry that routine, and docc keeps them from getting it wrong. Every link carries the version hash of what it points to. When a paragraph changes, every link to it breaks until an agent opens each place and checks it. When code changes, its fingerprint breaks until an agent checks it against the paragraphs. The build does not pass while any link is unchecked. There is no command that confirms everything at once: nothing can be confirmed without looking.

## Architecture

Pipeline: `loader` (strict JSON with offsets) → `schema` (built-in document schema + per-kind schemas from `docs/schemas`) → `engine` (reference graph, rules, suppressions, block codes and version hashes) → `render` (Markdown folder, single `.md`, single `.html`) → freshness comparison for `check`. `src/cli` is only flag parsing and dispatch (`mod.rs` `dispatch`, one `cmd_x` per command; `flags.rs` copies the behaviour of Go's `flag` package, which the CLI was first written with).

- **Rules** live in `src/rules.rs`; `explain`, `rules`, the skill and docs are derived from it. A new rule needs a registry entry, an `engine` check, a `testdata/<CODE>/` fixture and an `expected.json`. `skill_matches_registry_and_commands` fails if the skill mentions an unknown `DOCxxx` code or `docc <command>`; add new commands to its list in `tests/coverage.rs`.
- **Rendering**: HTML is built from the same Markdown: comrak parses it, `src/render/html/markdown.rs` writes goldmark-style HTML; `src/render/page.html`, `page.css`, `app.js` and `vendor/` are embedded with `include_str!`; `app_test.js` runs under Node from `cargo test` (skipped without Node). Page labels are written as `{{t:...}}` and need a Russian entry in `src/render/lang.rs` (a test fails on a missing or unused one). The Markdown output is never branded.
- **Text matching** (custom rules, DOC313) is done in NFC with positions mapped back to the source.
- **Block codes and version hashes** (`src/codes.rs`): every text block ends with a code ` ^PAY-14` (the document's `key`, a dash and the next number, `Numbers`), every link carries `@hash` of its target, code tags carry `~fingerprint`s; DOC209/DOC402 compare them (`src/engine/versions.rs`, `src/engine/trace.rs`), DOC212/DOC216 check codes. `docc build`/`docc codes` give missing codes, `docc migrate` (`src/engine/migrate.rs`) moves an old project; `src/engine/lock.rs` only reads an old `docc.lock` for it. Tests write documents in the short old form: `common::project` migrates them (codes fixed by `DOCC_CODE_SEED`), `raw_project`/`coded_project` keep the bytes.
- Tests are mostly golden: `testdata/<case>/` is a project plus `expected.json`; `tests/*.rs` build throwaway projects with `common::project([(path, content)])` and run the real binary in them (`p.run(&[...])`, `do_in`).

## Release and self-update

- `install.sh` / `install.ps1` and `docc update` (`src/cli/update.rs`) all resolve versions from `checksums.txt` of a GitHub Release (`latest/download/checksums.txt`, `download/<tag>/...`); the archive name `docc_<tag>_<os>_<arch>.<tar.gz|zip>` is the contract between `scripts/release.sh`, the installers and `ARCHIVE_RE` in `update.rs`. `DOCC_BASE_URL` points all three at a mirror for testing.
- `.github/workflows/release.yml` runs on tags `v*`. The version is injected at build time with the `DOCC_VERSION` environment variable (`option_env!`); a plain `cargo build` reports `dev` and `docc update` refuses it. The five targets are cross-built with `cargo zigbuild` (musl on Linux, gnu on Windows).
- The skill `skills/docc/SKILL.md` is installed by users with `npx skills add uxname/docc`; keep its frontmatter `name`/`description`. `skills/docc/evals/` holds fixture projects (its `code/` folders are sample source files for `trace`, not built) and `evals.md` explains how to run them.

## Working rules for this repo

- After any change: run `cargo test`, `docc build`, `docc check`, commit on `main`, and rebuild the installed binary with `cargo build --release && cp target/release/docc ~/.local/bin/docc`. Before replacing it, run the new build on `~/Документы/Work/o2` (`docc check --max-warnings 0`) because that project's hooks call `docc` from PATH.
- Skill and project documents are written in English; user-facing talk is in Russian.
- `docs/rust-port/*.md` (in Russian, for the owner) lists every behaviour that changed when docc moved from Go to Rust; add to it only if a Go-era behaviour is changed on purpose.
- Do not add CI beyond the two workflows without being asked; do not push or tag without being asked.
