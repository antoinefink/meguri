# Provenance and the clean room

Meguri reimplements Anki's behavior without its code (KTD1, R1954–R1960, R2021). Two roles keep the sources apart: spec writers read Anki and write specs in their own words; implementers build from those specs and never have Anki's source in their environment. This file says how that is recorded and checked. Role instructions are in `docs/process/roles/`.

| Role | Environment | Reads Anki? |
|---|---|---|
| spec-writer | full worktree or main checkout | yes, tag 26.09.3 only (`refs/anki-26.09.3`) |
| oracle-maintainer | full worktree or main checkout | yes, pinned tag and the `anki` wheel |
| implementer, fixer | sparse worktree without `oracle/`, deny profile | never |
| critic | packet directory, no tools (U10) | never |
| infra, integrator | main checkout | no need |

## Session records (R1958)

`docs/process/provenance/sessions.jsonl` holds one JSON line per agent session. Lines are only ever appended. `.gitattributes` merges the file with git's union driver, so records appended on different branches never conflict.

- `tools/mg worktree new --role <role> <name> [--base <ref>] [--branch <branch>] [--path <dir>]` creates the worktree (default branch `unit/<name>`, default path `<main checkout>-wt/<name>`), appends its record, and writes the same record to `<worktree git dir>/meguri-session.json`. Jobs read that marker to know a worktree's role. For implementer and fixer roles, git sets `extensions.worktreeConfig` in the repository config, so the sparse settings stay in that worktree.
- `tools/mg session record --role <role> [--name <name>] [--note <text>]` records a session that has no worktree (a critic packet, a spec writer in the main checkout).

Fields: `session` (id), `role`, `kind` (`worktree` or `session`), `name`, `base_commit`, `role_doc` and `role_doc_blob` (the role instructions in force), `created_at`; for worktrees also `branch`, `worktree`, `sparse`, `excluded_paths` and `deny_profile` (`source`, `installed`, `sha256` of the rendered profile).

A worktree made with plain `git worktree add` has no record, no marker and no deny profile. Create implementer worktrees only through `tools/mg worktree new`.

## Commit trailers

| Trailer | Required on |
|---|---|
| `Role:` one of spec-writer, implementer, fixer, oracle-maintainer, infra, critic, integrator | every commit touching `core/`, `app/` or `conformance/runner/`; checked for a known value wherever present |
| `Spec:` one or more spec ids, comma-separated or repeated (`specs/README-format.md`) | implementer and fixer commits touching those paths |
| `Session:` the session id | implementer and fixer commits touching those paths |

`tools/mg hooks install` installs a commit-msg shim in the repository's shared hooks directory. Every worktree then runs its own checkout's `tools/hooks/commit-msg`, which rejects a commit with a one-line reason. Merge commits are not checked. A `--no-verify` commit or an amend that only edits the message bypasses the hook; the merge gate (U19) checks the trailers of every commit on a branch.

## Similarity scan (R1957)

`tools/mg similarity` compares Meguri files with every Anki release tag from 2.1.50 to 26.09.3 (54 tags, about 6,800 unique blobs of `.rs .py .ts .svelte .js .css .scss .html .sql .proto .ftl`), plus AnkiConnect and Migaku add-on sources when they are under `refs/` (U135). It runs in Docker (`similarity` service) from the main checkout, which holds `refs/`; it refuses to run in an implementer or fixer worktree.

- Scope: `--base <ref>` scans files changed since that commit plus untracked files; `--rev <ref>` scans a commit's tree (for the merge gate); `-- <paths>` scans given files; no argument scans every file. `oracle/`, `docs/`, `specs/`, conformance data and build output are never scanned (`tools/similarity/config.yaml`).
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
- The corpus index is cached under `build/similarity-index/` and rebuilt when the tags, extra sources, parameters or interop vocabulary change (about 2 s).
- Calibration: `tools/mg similarity-selftest` runs the scanner tests and the calibration against the corpus. Positives are generated inside the container from the corpus and never written into the repository. Known overlap: six fsrs-rs files contain text taken from Anki (an Anki AGPL header in its benchmark, Anki's revlog documentation in its converter tests and simulator); the `com` channel flags them.
- Nightly: `tools/mg similarity-nightly` runs JPlag 6.3 with the latest tag, 2.1.66, 23.10, 24.06, 25.02 and 25.09 as old submissions and Meguri as the new one, per language, and warns at ≥ 10% similarity. Its `.jplag` reports embed the compared Anki sources: they stay in the main checkout's `build/` and open with `java -jar jplag.jar -M VIEW <file>` (JPlag 6 has no static HTML report).

## Residual paths

The clean room is evidence, not perfection. Known gaps:

- Git history still holds `oracle/`. Sparse checkout keeps the files off disk, and deny rules cover `git show`, `git log -p`, `git diff`, `git blame` on `oracle/` and `git cat-file`, but other git commands can still read those objects.
- Deny rules match path and command patterns; a differently spelled command can get around them. `WebFetch` rules work per domain, so Anki's repositories on github.com cannot be denied there without denying fsrs-rs too; Bash rules cover `ankitects`, `ankidroid`, AnkiConnect and AnkiWeb add-on URLs.
- A model may reproduce Anki code from memory. The similarity scan is the backstop, and its thresholds trade misses for noise.
- The similarity index under `build/` holds hashes plus Anki paths and line numbers, no text. JPlag reports embed Anki sources and stay in the main checkout.
