# Provenance and the clean room

Meguri reimplements Anki's behavior without its code (KTD1, R1954–R1960, R2021). Two roles keep the sources apart: spec writers read Anki and write specs in their own words; implementers build from those specs and never have Anki's source in their environment. This file says how that is recorded and checked. Role instructions are in `docs/process/roles/`.

| Role | Environment | Reads Anki? |
|---|---|---|
| spec-writer | full worktree or main checkout | yes, tag 26.09.3 only (`refs/anki-26.09.3`) |
| oracle-maintainer | full worktree or main checkout | yes, pinned tag and the `anki` wheel |
| implementer, fixer | sparse worktree without `oracle/`, deny profile | never |
| critic | packet directory, no tools (U10) | never |
| infra, integrator | main checkout | no need |

Antoine changed the build workflow on 2026-10-18: all new work happens directly on `master`, with no new branches or worktrees. Implementer and fixer sessions use `tools/mg session record`; their forbidden-source rules and commit trailers still apply. Existing role worktrees remain sources of prepared work. Their setup below describes the earlier workflow. Every completed unit requires its review log and verification receipts. Under Antoine’s updated goal on 2026-10-20, focused tests, independent review and provenance/privacy/similarity checks remain per unit. Full gates may cover up to 20 units changing only Rust internals; `docs/process/tiers.md` defines earlier boundaries, batch bases and receipts. Units remain awaiting acceptance until their required gate passes. Known failures block acceptance. Final suites and required human evidence stay mandatory; `tools/mg merge` is optional.

Manual device and reference checks are deferred until the final session (Antoine, 2026-10-20). Continue with automated VM tests. Final evidence remains required.

For new implementation and fixes, `tools/mg cleanroom` exports a filtered copy of
clean master into a separate container. Forbidden trees, build output, real Git
history, host credentials and dependency caches are absent from its source view.
Codex's command sandbox denies networking and access to the temporary run
credentials; web search and integrations are disabled. Synthetic boundary probes
must pass before each run. The orchestrator applies the reviewed patch to master
and verifies it through `tools/mg`. The integrator still has main-checkout access
and must not read forbidden sources. Details and runtime limitations:
`tools/cleanroom/README.md`. The earlier deny profiles below describe existing
worktrees; they are not filesystem isolation for new main-checkout sessions.

## Session records (R1958)

`docs/process/provenance/sessions.jsonl` holds one JSON line per agent session. Lines are only ever appended, and every record is committed where it is appended, in a commit that changes only that file (`Record session <id> (<role>, <name>)`, trailers `Role: infra` and `Session: <id>`). `.gitattributes` merges the file with git's union driver, so records committed on different branches never conflict, and no checkout is left with an uncommitted ledger that blocks a merge.

- `tools/mg worktree new --role <role> <name> [--base <ref>] [--branch <branch>] [--path <dir>]` creates the worktree (default branch `unit/<name>`, default path `<main checkout>-wt/<name>`), installs the commit-msg hook, writes the record to `<worktree git dir>/meguri-session.json` and commits it to the worktree's own ledger as the branch's first commit. The record reaches the integration branch when the unit merges. Jobs read the marker to know a worktree's role. For implementer and fixer roles, git sets `extensions.worktreeConfig` in the repository config, so the sparse settings stay in that worktree. If the marker or the record cannot be written or committed, the worktree (and a branch it created) is removed and the run is an error.
- `tools/mg session record --role <role> [--name <name>] [--note <text>]` records a session that has no worktree (a critic packet, a spec writer in the main checkout) and commits it on the current branch of the checkout it runs in. Other staged changes stay staged. Parallel runs (one critic session per check) wait for each other on a lock in the repository's git directory, so each record gets its own commit.
- Roles are matched as whole words: a value such as `implementer fixer` is rejected everywhere (worktree, session record, commit-msg hook).

Fields: `session` (id), `role`, `kind` (`worktree` or `session`), `name`, `base_commit`, `role_doc` and `role_doc_blob` (the role instructions in force), `created_at`; for worktrees also `branch`, `worktree`, `sparse`, `excluded_paths` and `deny_profile` (`source`, `installed`, `sha256` of the rendered profile).

A worktree made with plain `git worktree add` has no record, no marker and no deny profile. Create implementer worktrees only through `tools/mg worktree new`.

The deny profile binds a Claude session started in the worktree. An orchestrator's subagents start in the main checkout and never load it, so `.claude/hooks/cleanroom_guard.py` (a project PreToolUse hook in `.claude/settings.json`) applies the same rules to every subagent labelled as an implementer or fixer (workflow labels `U<n>:implement…`, `U<n>:fix-…`, `U<n>:codexfix-…`; Agent descriptions starting `implementer` or `fixer`). It also refuses git commands that would print `oracle/` content: unit branches carry the spec side's `oracle/` commits, which the sparse checkout keeps off disk but `git diff` and `git show` still print. Test: `.claude/hooks/test_cleanroom_guard.sh`.

## Commit trailers

| Trailer | Required on |
|---|---|
| `Role:` one of spec-writer, implementer, fixer, oracle-maintainer, infra, critic, integrator | every commit touching `core/`, `app/` or `conformance/runner/`; checked for a known value wherever present |
| `Spec:` one or more spec ids (`specs/README-format.md`) or Product Contract requirement ids (`R1987`), comma-separated or repeated | implementer and fixer commits touching those paths |
| `Session:` the session id | implementer and fixer commits touching those paths |

Which role and `Spec:` a commit uses:

- Work that reproduces Anki behavior: `Role: implementer` (or `fixer`) and the spec ids it implements.
- UI work that reproduces no Anki behavior (KTD1: built from the Product Contract alone): `Role: implementer` and the requirement ids, for example `Spec: R1987, R150`.
- Infrastructure units with no behavior to specify, such as the Rust workspace, FFI crate and Xcode project (U5–U7) and the tooling units: `Role: infra`. No `Spec:` or `Session:` is required. `Role: implementer` with `Spec: none` is rejected on code paths.

`tools/mg worktree new` installs the commit-msg hook for every role, and `tools/mg hooks install` installs it on its own; both put a shim in the repository's shared hooks directory. If a foreign `commit-msg` hook or `core.hooksPath` blocks it, `worktree new` still creates the worktree but reports `warn` with the reason. Every worktree runs its own checkout's `tools/hooks/commit-msg`, which rejects a commit with a one-line reason. Merge commits are not checked. A `--no-verify` commit or an amend that only edits the message bypasses the hook; the merge gate (U19) checks the trailers of every commit on a branch.

## Similarity scan (R1957)

`tools/mg similarity` compares Meguri files with every Anki release tag from 2.1.50 to 26.09.3 (54 tags, about 6,800 unique blobs of `.rs .py .ts .svelte .js .css .scss .html .sql .proto .ftl`), plus AnkiConnect and Migaku add-on sources when they are under `refs/` (U135). It runs in Docker (`similarity` service) from the main checkout, which holds `refs/`; it refuses to run in an implementer or fixer worktree.

- The corpus is pinned (`corpus.tags` in `config.yaml`: range and count). A corpus that lacks a release in the range, or has more than the pinned count, is an error, not a pass.
- Scope: `--base <ref>` scans files changed since that commit plus untracked files; `--rev <ref>` scans a commit's tree (for the merge gate); `-- <paths>` scans given files, relative to the checkout (`./x`, `a/../x` and an absolute path inside it count as `x`; a path outside it is an error); no argument scans every file. `oracle/`, `docs/`, `specs/`, conformance data and build output are never scanned (`tools/similarity/config.yaml`). File lists are NUL-separated, so names with any character are scanned. A listed file in scope that is not there makes the run `warn`. `--rev` checks out only the files outside those excluded directories (plus the interop vocabulary) under the run directory and deletes them after the scan.
- Channels and thresholds (`config.yaml`):
  - `struct`: tokens with identifiers abstracted, k=40, w=10. Flags at ≥ 10% matching fingerprints (with ≥ 30 fingerprints) or a run of ≥ 6 consecutive matching fingerprints.
  - `lex`: identifier words and literals, k=12. Flags at ≥ 15% (with ≥ 30 fingerprints).
  - `com`: any shared run of 10 comment words.
  - `text`: any shared run of 8 words between String Catalog English values or Meguri CSS and Anki's Fluent messages, CSS and string literals (R1959).
  - Swift is scanned through `lex` and `com` only.
- Two noise filters came out of calibration. Rust attributes (`#[...]`) count as one token. `struct` k-grams that repeat a short period (field lists, column lists, row decoding; periodicity ≥ 0.8) are not fingerprinted. Without them, independent storage code and unrelated fsrs-rs structs flagged on field lists alone.
- Interop masking: words listed in `specs/<area>/interop-vocab.txt` are dropped from the `lex` channel, in code and in string literals, on both sides. A flag that survives masking is escalated.
- Reports: the implementer view (`implementer.md` and `.json`: file, lines, channel, score, "rewrite this region from the spec") goes to the implementer. The reviewer view adds the matching Anki source, tag, path and lines, for human review only. Neither view contains corpus text.
- Allowlist: `tools/similarity/allowlist.yaml` exempts interop facts (schema-18 DDL, protobuf field declarations, sync endpoint and JSON field names, config keys, search keywords) by path, optional line range and channels. Each entry needs a `reason` and `approved_by: antoine`, and is written only from Antoine's chat approval, recorded as an escalation (U19).
- The corpus index is cached under `build/similarity-index/` and rebuilt when the tags, extra sources, parameters or interop vocabulary change (about 2 s). Concurrent scans share the cache: each build writes its own directory, a lock covers publishing and opening, and the three most recently used indexes are kept.
- Calibration: `tools/mg similarity-selftest` runs the scanner tests and the calibration against the corpus. Positives are generated inside the container from the corpus and never written into the repository. Known overlap: six fsrs-rs files contain text taken from Anki (an Anki AGPL header in its benchmark, Anki's revlog documentation in its converter tests and simulator); the `com` channel flags them.
- Nightly: `tools/mg similarity-nightly` runs JPlag 6.3 with the latest tag, 2.1.66, 23.10, 24.06, 25.02 and 25.09 as old submissions and Meguri as the new one, per language, and warns at ≥ 10% similarity. Its `.jplag` reports embed the compared Anki sources: they stay in the main checkout's `build/` and open with `java -jar jplag.jar -M VIEW <file>` (JPlag 6 has no static HTML report).

## Residual paths

The clean room is evidence, not perfection. Known gaps:

- Git history still holds `oracle/`. Sparse checkout keeps the files off disk, and deny rules cover `git show`, `git log -p`, `git diff` and `git blame` naming `oracle`, and every `git cat-file`, but other git commands (`git grep`, `git archive`, a `git log -p` without a path) can still read those objects.
- Deny rules match the command text against patterns; a differently spelled command can get around them, for example a relative path to the main checkout or a URL with other capitalization. Rules for a command start and end with a wildcard and put one before each subcommand, so options in front (`git -C <dir> show`, `docker compose -f <file> exec`) or a wrapper (`env`, `cd x &&`) do not defeat them. Bash rules cover the absolute paths of `refs/`, `local-data/` and every `oracle/`, the main checkout's similarity reports and index (`build/mg/similarity*`, `build/similarity-index`), reviewer views and `.jplag` reports anywhere, the installed Anki app and add-ons, and Anki, AnkiDroid, AnkiConnect, Migaku and AnkiWeb add-on URLs.
- Implementers run containers only through `tools/mg`, whose subprocesses the rules do not see. Bash rules deny any direct `docker` command that runs, execs into, debugs, builds, copies from, saves or exports a container or image, and `docker-compose`.
- `WebFetch` rules work per domain: github.com and its content hosts are denied outright (implementers read fsrs-rs on docs.rs or from its crate on static.crates.io). The `tools/hooks/deny-quarantined-fetch` hook blocks Anki, AnkiDroid, AnkiConnect and Migaku URLs on any other host (mirrors, CDNs, code search). Other copies under unrelated names stay reachable.
- A model may reproduce Anki code from memory. The similarity scan is the backstop, and its thresholds trade misses for noise.
- The similarity index under `build/` holds hashes plus Anki paths and line numbers, no text. JPlag reports embed Anki sources and stay in the main checkout.
