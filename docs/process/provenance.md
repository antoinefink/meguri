# Provenance and the clean room

Meguri reimplements Anki's behavior without its code (KTD1, R1954–R1960, R2021). Two roles keep the sources apart: spec writers read Anki and write specs in their own words; implementers build from those specs and never have Anki's source in their environment. This file says how that is recorded and checked. Role instructions are in `docs/process/roles/`.

| Role | Environment | Reads Anki? |
|---|---|---|
| spec-writer | full worktree or main checkout | yes, tag 26.09.3 only (`refs/anki-26.09.3`) |
| oracle-maintainer | full worktree or main checkout | yes, pinned tag and the `anki` wheel |
| implementer, fixer | sparse worktree without `oracle/`, deny profile | never |
| critic | packet directory, no tools (U10) | never |
| infra, integrator | task or main checkout | no need |

Work may use a task branch or a coordinated main checkout under [the integration workflow](merge.md). Record the role before implementation. Each completed unit requires a review log, focused tests, independent review, and provenance, privacy and similarity evidence. [Verification tiers](tiers.md) govern batch bases, gate receipts and acceptance.

Manual device and reference checks are deferred until the final session (Antoine, 2026-10-20). Continue with automated VM tests. Final evidence remains required.

The sole interim exception to known failures blocking acceptance is Antoine's 2026-10-21 R554 decision, approval `E-20261021T134124Z-a07a`. Its exact scope is in `tiers.md`: until U69, retain the failed full gate and enabled test, require every other check, and record the exception per unit. M1R's technical stop condition and final acceptance requirements remain.

For new implementation and fixes, `tools/mg cleanroom` exports a filtered copy of
a clean, committed checkout into a separate container. Its source view excludes
forbidden trees, build output, real Git history, host credentials and dependency caches.
Codex's command sandbox denies networking and access to the temporary run
credentials; web search and integrations are disabled. Synthetic boundary probes
must pass before each run. The orchestrator applies the reviewed patch to the task
checkout and verifies it through `tools/mg`. The integrator has main-checkout access
and must respect role source boundaries. The deny profiles below are an additional
worktree control. See `tools/cleanroom/README.md` for runtime isolation.

## Session records (R1958)

Each agent session has one file, `docs/process/provenance/sessions/<id>.json`, holding one JSON object. Records are only ever added, never edited or removed, and every record is committed where it is created, in a commit that changes only that file (`Record session <id> (<role>, <name>)`, trailers `Role: infra` and `Session: <id>`). Two branches never touch the same file, so their merges, GitHub's included, never conflict on records, and no checkout is left with an uncommitted record.

`docs/process/provenance/sessions.jsonl` is the archive of the records made before the switch (2026-11-20), one per line. Nothing appends to it now; readers take the archive and the directory (`mg_session_records`). Transition rule: a branch still on the old tools (its record commit's parent has no `tools/gate/static-checks/sessions.py`) may add archive lines, each in a commit that changes only the archive, with those trailers and valid fields, so it passes whenever it merges. `.gitattributes` keeps the union driver for local merges of the archive; GitHub still shows two such branches as conflicting with each other. Any other new archive line fails the check: move it, by removing the line in one commit and committing the same record (same session id, any invalid field corrected) alone as `sessions/<id>.json`. Appended lines go at the end; admitted bytes never change. Follow-up: drop the allowance once no open branch predates the switch.

`tools/mg static-checks` (`sessions`, also in the merge gate) checks every commit of a branch and the branch against its merge bases: a record file is only added, as a regular file, alone, with those trailers, a known role, its role's `role_doc` and `role_doc_blob` at a full `base_commit` id the commit descends from; no archive line is removed or changed; a merge neither drops, changes nor adds a record; each session id is recorded once; every `Session:` trailer names a record committed at the tip or the base.

- `tools/mg worktree new --role <role> <name> [--base <ref>] [--branch <branch>] [--path <dir>]` creates the worktree (default branch `unit/<name>`, default path `<main checkout>-wt/<name>`), installs the commit-msg hook, writes the record to `<worktree git dir>/meguri-session.json` and commits it in the worktree as the branch's first commit. The record reaches the integration branch when the unit merges. Jobs read the marker to know a worktree's role. For implementer and fixer roles, git sets `extensions.worktreeConfig` in the repository config, so the sparse settings stay in that worktree. If the marker or the record cannot be written or committed, the worktree (and a branch it created) is removed and the run is an error.
- `tools/mg session record --role <role> [--name <name>] [--note <text>]` records a session in an existing checkout and commits the record on its current branch. Other staged changes stay staged. Parallel runs wait for each other on a lock in the repository's git directory, so each record gets its own commit.
- Roles are matched as whole words: a value such as `implementer fixer` is rejected everywhere (worktree, session record, commit-msg hook).
- Names are plain: a worktree name matches `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`, a `session record --name` `[A-Za-z0-9][A-Za-z0-9 ._-]{0,79}`. The pre-commit private-token scan reads a new record like any added file. When it refuses one (an ordinary word in a name, note or path can be in the private word list), nothing is recorded, a new worktree is removed, and the run fails with `session-private`: pick another name, note, branch or path. If the message says the record is still staged (a locked index), unstage and remove it as it says first. Neither the hook nor the message names the word; never look it up.

Fields: `session` (id), `role`, `kind` (`worktree` or `session`), `name`, `base_commit`, `role_doc` and `role_doc_blob` (the role instructions in force), `created_at`; for worktrees also `branch`, `worktree`, `sparse`, `excluded_paths` and `deny_profile` (`source`, `installed`, `sha256` of the rendered profile).

Use `tools/mg worktree new` to set up a dedicated role worktree. An orchestrator using an existing checkout can record the session with `tools/mg session record` and run the filtered implementation there.

The deny profile binds a Claude session started in the worktree. An orchestrator's subagents start in the main checkout and never load it, so `.claude/hooks/cleanroom_guard.py` (a project PreToolUse hook in `.claude/settings.json`) applies the same rules to every subagent labelled as an implementer or fixer (workflow labels `U<n>:implement…`, `U<n>:fix-…`, `U<n>:codexfix-…`; Agent descriptions starting `implementer` or `fixer`). It also refuses git commands that would print `oracle/` content: unit branches carry the spec side's `oracle/` commits, which the sparse checkout keeps off disk but `git diff` and `git show` still print. Test: `.claude/hooks/test_cleanroom_guard.sh`.

## Routine privacy admissions

Antoine delegates the following routine updates across Meguri work until revoked
(2026-11-16). Qualifying updates need no manual reconfirmation.

For session records (`docs/process/provenance/sessions/<id>.json`, and lines
the transition rule above still adds to the archive `sessions.jsonl`):

- An independent non-author reviews every new record, including descriptions
  and paths, and confirms that it contains only public operational metadata.
- The orchestrator preserves the previously admitted bytes and original
  record-only commits. For the archive, it verifies the exact candidate digest.
  A record file needs no digest entry: the pre-commit scan passes it whole, or
  `tools/mg` refuses it and the session is recorded under another name.

For public-file snapshots, including first admissions and later hash refreshes:

- An independent non-author reviews the complete candidate contents, including
  comments, literals, embedded data and paths. Reviewing only flagged lines or
  changed lines is insufficient. Changed bytes require a new complete review.
- The review uses permitted public material, without opening private inputs or
  inspecting matched private tokens. Existing role boundaries still apply.
- Existing authorship, dependency-license and clean-room evidence must support
  the file. Privacy classification alone provides none of those clearances.
- The orchestrator verifies the reviewed bytes and exact repository-relative path.
  Add or refresh only that path's whole-file SHA-256 in the existing public-artifact map.
  Never add path patterns, partial-content exemptions or caller-controlled manifests.

For either update, record the reviewer, reviewed digest and decision in existing
review evidence. Preserve original history and run the required privacy,
provenance, similarity and focused checks. Future changed bytes remain checked.

Resolve ordinary review findings autonomously. Escalate actual or uncertain
private content, unresolved authorship or licensing concerns, historical ledger
edits, unfamiliar formats, and changes to matcher behavior, thresholds,
private-data access, similarity exceptions or clean-room rules.
This delegation covers only the exact hash entries above. Use normal tool
approval; it cannot override a denial or an owner-only action.

## Commit trailers

| Trailer | Required on |
|---|---|
| `Role:` one of spec-writer, implementer, fixer, oracle-maintainer, infra, critic, integrator | every commit touching `core/`, `app/`, `server/` or `conformance/runner/`; checked for a known value wherever present |
| `Spec:` one or more spec ids (`specs/README-format.md`) or Product Contract requirement ids (`R1987`), comma-separated or repeated | implementer and fixer commits touching those paths |
| `Session:` the session id | implementer and fixer commits touching those paths |

Which role and `Spec:` a commit uses:

- Work that reproduces Anki behavior: `Role: implementer` (or `fixer`) and the spec ids it implements.
- UI work that reproduces no Anki behavior (KTD1: built from the Product Contract alone): `Role: implementer` and the requirement ids, for example `Spec: R1987, R150`.
- Infrastructure units with no behavior to specify, such as the Rust workspace, FFI crate and Xcode project (U5–U7) and the tooling units: `Role: infra`. No `Spec:` or `Session:` is required. `Role: implementer` with `Spec: none` is rejected on code paths.

`tools/mg worktree new` installs the commit-msg hook for every role, and `tools/mg hooks install` installs it on its own; both put a shim in the repository's shared hooks directory. If a foreign `commit-msg` hook or `core.hooksPath` blocks it, `worktree new` still creates the worktree but reports `warn` with the reason. Every worktree runs its own checkout's `tools/hooks/commit-msg`, which rejects a commit with a one-line reason. Merge commits are not checked. A `--no-verify` commit or an amend that only edits the message bypasses the hook; the merge gate (U19) checks the trailers of every commit on a branch.

## Similarity scan (R1957)

`tools/mg similarity` compares Meguri files with every Anki release tag from 2.1.50 to 26.09.3 (54 tags, about 6,800 unique blobs of `.rs .py .ts .svelte .js .css .scss .html .sql .proto .ftl`), plus AnkiConnect and Migaku add-on sources when they are under `refs/` (U135). It runs in Docker (`similarity` service) from the main checkout, which holds `refs/`; it refuses to run in an implementer or fixer worktree.

The gate is the hosted check **Linux · similarity** (owner's decision, 2026-11-21). On a pull request it fetches the pinned tags from github.com/ankitects/anki into a directory outside the checkout, scans the files the change adds or modifies against its base, with the base's allowlist (`--base <base> --rev HEAD --allowlist-rev <base>`), and checks the test data of the conformance areas it touches. It fails when the scanner errs or flags a file; a human or the coordinator then decides. Its log and step summary hold only file names, scores and counts; it uploads nothing. Its index cache holds hashes, Anki paths and line numbers, no text; but the `com` and `text` hashes cover every 10- and 8-word run, so text could be recovered from them with effort, and any run of this repository's workflows can restore master's entry. A change to the `channels:` or `scan:` section of `config.yaml` fails the check: a human reviews it. The coordinator no longer runs the scan by hand before a merge. The local scan stays for the reviewer view, which names the matching Anki sources and never leaves the main checkout.

- The corpus is pinned (`corpus.tags` in `config.yaml`: range and count; `corpus.commits`: each tag's commit). A corpus that lacks a release in the range, has more than the pinned count, or whose tags point at other commits is an error, not a pass.
- Scope: `--base <ref>` scans files changed since that commit plus untracked files; `--rev <ref>` scans a commit's tree (for the merge gate); `--allowlist-rev <ref>` uses that commit's allowlist; `-- <paths>` scans given files, relative to the checkout (`./x`, `a/../x` and an absolute path inside it count as `x`; a path outside it is an error); no argument scans every file. `oracle/`, `docs/`, `specs/`, conformance data and build output are never scanned (`tools/similarity/config.yaml`). File lists are NUL-separated, so names with any character are scanned. A listed file in scope that is not there makes the run `warn`. `--rev` checks out only the files outside those excluded directories (plus the interop vocabulary) under the run directory and deletes them after the scan.
- Channels and thresholds (`config.yaml`):
  - `struct`: tokens with identifiers abstracted, k=40, w=10. Flags at ≥ 10% matching fingerprints (with ≥ 30 fingerprints) or a run of ≥ 6 consecutive matching fingerprints.
  - `lex`: identifier words and literals, k=12. Flags at ≥ 15% (with ≥ 30 fingerprints).
  - `com`: any shared run of 10 comment words.
  - `text`: any shared run of 8 words between String Catalog English values or Meguri CSS and Anki's Fluent messages, CSS and string literals (R1959).
  - Swift is scanned through `lex` and `com` only. Server Rust and JS/JSX/TS/TSX use their existing language channels. Similarity results do not prove translation independence.
- Two noise filters came out of calibration. Rust attributes (`#[...]`) count as one token. `struct` k-grams that repeat a short period (field lists, column lists, row decoding; periodicity ≥ 0.8) are not fingerprinted. Without them, independent storage code and unrelated fsrs-rs structs flagged on field lists alone.
- Interop masking: words listed in `specs/<area>/interop-vocab.txt` are dropped from the `lex` channel, in code and in string literals, on both sides. A flag that survives masking is escalated.
- Reports: the implementer view (`implementer.md` and `.json`: file, lines, channel, score, "rewrite this region from the spec") goes to the implementer. The reviewer view adds the matching Anki source, tag, path and lines, for human review only. Neither view contains corpus text.
- Allowlist: `tools/similarity/allowlist.yaml` exempts interop facts (schema-18 DDL, protobuf field declarations, sync endpoint and JSON field names, config keys, search keywords) by path, optional line range and channels. Each entry needs a `reason` and `approved_by: antoine`, and is written only from Antoine's chat approval, recorded as an escalation (U19).
- The corpus index is cached under `build/similarity-index/` and rebuilt when the tags, extra sources, parameters or interop vocabulary change (about 2 s). Concurrent scans share the cache: each build writes its own directory, a lock covers publishing and opening, and the three most recently used indexes are kept.
- Calibration: `tools/mg similarity-selftest` runs the scanner tests and the calibration against the corpus. Positives are generated inside the container from the corpus and never written into the repository. Known overlap: six fsrs-rs files contain text taken from Anki (an Anki AGPL header in its benchmark, Anki's revlog documentation in its converter tests and simulator); the `com` channel flags them.
- Test data (R1881): `tools/mg similarity --test-data -- <paths>` (old name `--search-data`) scans conformance scenarios and goldens through `lex`, `com` and `text`, which the ordinary scan never reads. Paths are `conformance/scenarios/<area>` and `conformance/goldens/<area>` of any area, or the two roots for all areas; every scenario needs the golden of the same name in the same area. Files below the fingerprint limits are reported as limited: the lane says nothing about them. The hosted job scans each touched area whole and fails only on flags in files the change adds or modifies; other flags are reported as findings on the base. Recorded oracle output is not a finding (standing decision of 2026-11-21): a flagged golden is reported as `oracle-output`, with its flags, and does not fail the scan when it has exactly the shape `tools/mg oracle record` writes (the six top-level keys, `oracle` with its three, a short `tz` and `clock`), names the current pin (`oracle.anki_version` and `oracle.lock_sha256` equal to `conformance/oracle.json`, which `scan.oracle_goldens.pin` names), carries its own path as `scenario_id`, and its `scenario_sha256` is the D3 hash of the paired scenario as it is now. Scenarios are never exempt, nor is a golden of another shape, pin or scenario state. The header can be copied, so the exemption still trusts the rule that only `oracle record` writes goldens; the T1 `oracle compare` is the check of the observations themselves. On a pull request a change to the rule or to `conformance/oracle.json` trips the hosted job's settings guard. An Anki test case copied into a scenario's inputs is still a finding when its golden is exempt: its trace was often only Anki's asserted output in the golden, so the reviewer view lists `oracle-output` files for a human to look at.
- Nightly: `tools/mg similarity-nightly` runs JPlag 6.3 with the latest tag, 2.1.66, 23.10, 24.06, 25.02 and 25.09 as old submissions and Meguri as the new one, per language, and warns at ≥ 10% similarity. Its `.jplag` reports embed the compared Anki sources: they stay in the main checkout's `build/` and open with `java -jar jplag.jar -M VIEW <file>` (JPlag 6 has no static HTML report).

## Residual paths

The clean room is evidence, not perfection. Known gaps:

- Git history still holds `oracle/`. Sparse checkout keeps the files off disk, and deny rules cover `git show`, `git log -p`, `git diff` and `git blame` naming `oracle`, and every `git cat-file`, but other git commands (`git grep`, `git archive`, a `git log -p` without a path) can still read those objects.
- Deny rules match the command text against patterns; a differently spelled command can get around them, for example a relative path to the main checkout or a URL with other capitalization. Rules for a command start and end with a wildcard and put one before each subcommand, so options in front (`git -C <dir> show`, `docker compose -f <file> exec`) or a wrapper (`env`, `cd x &&`) do not defeat them. Bash rules cover the absolute paths of `refs/`, `local-data/` and every `oracle/`, the main checkout's similarity reports and index (`build/mg/similarity*`, `build/similarity-index`), reviewer views and `.jplag` reports anywhere, the installed Anki app and add-ons, and Anki, AnkiDroid, AnkiConnect, Migaku and AnkiWeb add-on URLs.
- Implementers run containers only through `tools/mg`, whose subprocesses the rules do not see. Bash rules deny any direct `docker` command that runs, execs into, debugs, builds, copies from, saves or exports a container or image, and `docker-compose`.
- `WebFetch` rules work per domain: github.com and its content hosts are denied outright (implementers read fsrs-rs on docs.rs or from its crate on static.crates.io). The `tools/hooks/deny-quarantined-fetch` hook blocks Anki, AnkiDroid, AnkiConnect and Migaku URLs on any other host (mirrors, CDNs, code search). Other copies under unrelated names stay reachable.
- A model may reproduce Anki code from memory. The similarity scan is the backstop, and its thresholds trade misses for noise.
- The similarity index under `build/` holds hashes plus Anki paths and line numbers, no text. JPlag reports embed Anki sources and stay in the main checkout.
- The hosted job runs the pull request's own workflow and scripts, as every hosted check does: a change to them can change what it prints or how it decides. Review changes to `.github/similarity.sh`, the job and `tools/similarity/` with that in mind.
