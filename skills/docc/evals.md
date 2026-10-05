# docc skill: evaluation set

Six small tasks that check whether an agent with the `docc` skill does the right thing on a real project, not only whether `docc check` ends green. Each task has a fixture project in `evals/<n>/`, a prompt in Russian (`evals/<n>/prompt.txt`, written as a user would type it) and a pass script (`evals/<n>/check.sh`).

Every fixture except 6 starts clean: `docc check --max-warnings 0` passes, `docs/out` and `docc.lock` are built.

## How to run

For each task, in a fresh temporary copy of the fixture (without `prompt.txt` and `check.sh`):

1. Install the skill as a project skill: `.claude/skills/docc/SKILL.md`.
2. `git init` and commit the fixture, so code tags (files from `git ls-files`) and `git diff` work.
3. Put the docc binary built from this repo first in `PATH`.
4. Run the agent (10 minutes limit):

   ```sh
   claude -p "$(cat prompt.txt)" --setting-sources project --strict-mcp-config --permission-mode acceptEdits \
     --allowedTools "Bash Read Edit Write Glob Grep Skill" --output-format stream-json --verbose
   ```

   `Skill` is added to the allowed tools so the headless run can load the skill without a permission prompt.
5. Save the final answer of the agent into `answer.txt` in the copy and run `FIX=<fixture dir> <fixture dir>/check.sh` in the copy. Every check starts with `docc check --max-warnings 0` and then tests one specific result. It prints `PASS` and exits with 0 on success.

The project has no Claude Code hooks: the eval measures what the skill text alone makes the agent do.

## Tasks

### 1. A document of a kind that does not exist yet

- Setup `evals/1`: kind `page` only; documents `index` and `api` (HTTPS and JSON, no endpoints).
- Prompt: «Опиши в документации эндпоинт POST /api/orders: он создаёт заказ из списка товаров (у каждого товара артикул и количество) и возвращает код 201 и номер заказа; если список товаров пустой, возвращает 400. Это первый из многих эндпоинтов, у всех будут метод, путь и коды ответов, и я хочу, чтобы docc проверял эти поля.»
- Pass: the document that mentions `/api/orders` has a kind other than `page`; `docs/schemas/<kind>.json` sets a boolean `additionalProperties`; the final answer names the kind.

### 2. A changed fact behind a referenced anchor

- Setup `evals/2`: `security-policy` has the paragraph «… блокирует вход на 15 минут. ^lockout». `login-screen` links to it and repeats «Вход заблокирован на 15 минут». `code/auth.go` has a region `docc:begin security-policy#^lockout` with `lockoutDuration = 15 * time.Minute`.
- Prompt: «Заказчик поменял правило: после пяти неверных паролей подряд вход теперь блокируется на 30 минут, а не на 15. Обнови документацию проекта.»
- Pass: the anchored paragraph and `login-screen` say «30 минут», «15 минут» is gone from `docs/src`, the code says `lockoutDuration = 30 * time.Minute` and the region is still there. `docc check` (no DOC209, no DOC402) proves `docc ack` ran after the review.

### 3. Link a code function to a requirement paragraph

- Setup `evals/3`: `req-export` with the paragraphs `^csv-format` and `^file-name`; `code/export.go` with `FileName` and `WriteCSV`; `trace.scan` is on, no tags yet.
- Prompt: «Свяжи функцию WriteCSV из code/export.go с требованием о формате CSV в документации так, чтобы при изменении кода этой функции docc показывал, что требование надо перепроверить.»
- Pass: a `docc:begin … req-export#^csv-format` … `docc:end` region contains `func WriteCSV` and not `func FileName`; `docc.lock` has the line `code/export.go req-export#^csv-format <hash> <code hash>`.

### 4. Rename a section that others link to

- Setup `evals/4`: `billing` has the field `rules` with `$sections` «Оплата картой» and «Возврат средств». `faq`, `support` and a code tag in `code/refund.go` link to `billing#rules/Возврат средств`.
- Prompt: «Переименуй в документе «Оплата» раздел «Возврат средств» в «Возврат и отмена заказа».»
- Pass: «Возврат средств» is gone from `docs/src` and `code`; `billing.json` has the section «Возврат и отмена заказа»; three files (`faq`, `support`, `code/refund.go`) link to `billing#rules/Возврат и отмена заказа`. `docc check` proves there is no DOC312 and no unconfirmed DOC209.

### 5. A product spec with numbering: a new page between two others

- Setup `evals/5`: no code, `"numbering": true`, `lang: ru`. Children of `index`: «Цели» (order 1), «Пользователи» (2), «Требования» (3), «Термины» (4).
- Prompt: «Добавь в ТЗ раздел «Ограничения» между «Пользователи» и «Требования». Содержание: сайт работает в браузерах Chrome, Firefox и Safari; мобильное приложение в первую версию не входит.»
- Pass: children of `index` sorted by `order`, then title, read «Цели | Пользователи | Ограничения | Требования | Термины»; no title starts with a digit. `docc check` proves there is no DOC314.

### 6. Fix a project with several errors

- Setup `evals/6`: a strict kind `requirement` (`additionalProperties: false`, fields `statement`, `priority`, `owner`). `req-login` has `"priorty": "high"` (unknown field, required `priority` missing) and `"owner": "N/A"`; `req-reset` links to `[[req-logn]]`. `docs/out` was built with `owner: N/A` already in place, so the real owner cannot be recovered from the output; the lock is older, so DOC209 also shows up.
- Prompt: «docc check падает. Почини документацию.»
- Pass: `req-login` has `priority: high` and no `priorty`; `req-reset` links to `[[req-login]]` and `req-logn.json` was not created; «N/A» is gone outside `notes` and `req-login` has no `owner` (the owner is unknown, so a guessed value fails; the doubt belongs in `notes`); no `docIgnore`; `requirement.json` is unchanged.

## Results (2026-10-05)

One headless run per task, model `claude-opus-5-5`, Claude Code 2.1.289, docc built from commit `2184fed`. Every run loaded the skill with the `Skill` tool as its first step. Runs took 23 to 51 seconds and 5 to 10 turns.

| Task | Result | Notes |
|---|---|---|
| 1. New kind | pass | Created a strict `docs/schemas/endpoint.json` (`additionalProperties: false`, `method` enum, `path` pattern, `responses` with a code pattern) and `create-order.json` under `api`. Tested the schema with bad values in a temporary copy. Put the missing facts (JSON field names, order number format) into `notes` instead of inventing them. Named the new kind in the answer. |
| 2. Changed fact | **fail** | Updated the anchored paragraph and `login-screen`, read both sides, acked only the links it had checked. Left `lockoutDuration = 15 * time.Minute` in `code/auth.go` and the DOC209 from the code region open, on the grounds that the user asked to update "the documentation" and the change alters product behaviour. The answer reported this honestly and offered to change the code. |
| 3. Code region | pass | Wrapped the doc comment and `WriteCSV` in `docc:begin req-export#^csv-format` … `docc:end`; checked that a change of the code raises DOC402, then restored the file. The backup went to `/tmp` outside the project. |
| 4. Rename section | pass | Renamed the key, fixed `faq`, `support` and the code tag with one `sed`, read the sources and ran `docc ack billing` (plus redundant `ack faq` and `ack support`). |
| 5. Numbered spec | pass | New `constraints.json` with `order: 3`; shifted «Требования» to 4 and «Термины» to 5 instead of using a value between neighbours. Grepped for typed section numbers in text before finishing. |
| 6. Several errors | pass (second run) | First run: the fixture leaked the owner through `docs/out`, and the agent restored «Команда авторизации» from there (citing the source). The fixture was rebuilt so the output also says `N/A`. Second run: renamed `priorty`, fixed the link using `docc.lock` as evidence, removed `owner` and put the doubt into `notes`, read `req-login` before `docc ack req-login`, no `docIgnore`. |

Score: 5 of 6.

Problems in the eval itself, already fixed in the fixtures and checks: fixture 6 kept the original owner in `docs/out`; the first check of task 6 also matched «N/A» quoted in `notes`. Every answer ended with a paragraph about unauthenticated claude.ai connectors: add `--strict-mcp-config` to the run command so that user MCP servers do not leak into the eval.

### Suggested fixes to SKILL.md

1. Task 2. The rule that code must follow a changed decision is only in "Before you finish" (item 6), and the agent overrode it with its own scope reading. In "Changing a decision" and in the DOC209 bullet, say it directly: "When the user states a new fact or decision, code inside a `docc:begin` region or tagged with `docc:` that holds the old value is part of the same change: update it, read it, then `docc ack`. Leave it only when the user explicitly says not to touch code; then do not ack that link and name the open DOC209 in the report." Item 6 of "Before you finish" can then point to that section.
2. Task 5. Add to the `order` bullet: "To put a page between two siblings, give it an `order` between theirs (for example `2.5`), so the other files stay unchanged; renumber the siblings only when the user asks for round numbers." (Shifting is not wrong, but it touches more files and more lock hashes.)
3. Task 6. The DOC202 hint says "Create docs/src/req-logn.json or fix the id"; the agent did the right thing, but the skill can back it up in rule 2: "For DOC202 look for the intended id first (a near spelling, the old line in `docc.lock`)."
4. Task 4. `docc ack X` confirms lines where X is the source, the target or the target's document. Add one line: "After a change of one target, `docc ack <changed id>` is enough; acking the sources too is not needed."
5. Minor: the strict schema in task 1 copied the shape of the skill's `endpoint` example (same kind name, `x-docc-target` on `parent`, `method` enum). The name came from the user's word «эндпоинт», so it is fine here, but an example kind that is less likely to match real requests (for example `report`) would show better whether the agent designs a schema from the user's words.

### Rerun after the skill fixes (2026-10-05)

SKILL.md now says that code in a linked region is part of the same change, that one `docc ack` of the changed target is enough, that a page goes between two others with an `order` between theirs, and that a broken link is first matched against a near id or the old target in `docc.lock`. Task 2 rerun: PASS (the code holds `30 * time.Minute`, the region stays, no DOC209 or DOC402). `--strict-mcp-config` keeps the user's connectors out of the run.
