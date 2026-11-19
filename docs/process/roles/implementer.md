# Role: implementer

You build Meguri from specs. Anki's source never enters your environment (KTD1, R1955). Fixers, who work an implementer's review findings, follow this file too.

## Session

- The orchestrator chooses the task checkout under `docs/process/merge.md`. Record an implementer or fixer session there. Follow the source exclusions for files, history and reports. See `docs/process/tiers.md` for unit checks and integration evidence.

- Manual device and reference checks are deferred until the final session (Antoine, 2026-10-20). Continue with automated VM tests. Final evidence remains required.

- Implement and fix through `tools/mg cleanroom --prompt <file> --session <id>` using the clean task checkout. The orchestrator reviews the patch, applies it to that checkout and runs verification there. See `tools/cleanroom/README.md`.

- In the filtered runner, implement the change, correct obvious source errors, make one consistency check and export promptly. The integrator owns native compilation, focused verification and independent review.

- Put the complete handoff in the final response: the filtered runner exports `codex.out`, but files in its `/tmp` disappear when it exits. A report at `/work`'s root fails the source export guard.

- Use `tools/mg worktree new --role implementer <name>` (or `--role fixer`) for a dedicated role worktree. It installs the deny profile and records the session. In an existing task checkout the orchestrator can record the filtered-runner session with `tools/mg session record`.
- `refs/` and `local-data/` are never in a worktree. Do not reach the main checkout's copies by absolute path.

## You may read

- `specs/`, `conformance/` (schema, scenarios, goldens, fixtures, catalog), the plan and its research dossiers under `docs/plans/` (not the private research notes in `local-data/research/`).
- Oracle outputs, by running the oracle as a black box through `tools/mg` jobs. Run containers only through `tools/mg`: the deny profile refuses direct `docker` commands that run, exec into, build or copy from a container or image.
- fsrs-rs source, on docs.rs (source view) or from its crate on static.crates.io; GitHub is denied to implementer sessions. Some fsrs-rs files carry text that came from Anki (its revlog converter tests, simulator types and a benchmark header). Depend on the crate; never copy from fsrs-rs either.
- Meguri's own code and docs.

## You never read

- `oracle/` (driver, harvest links, provenance notes), or its history (`git show`, `git log -p` and the like on `oracle/`).
- `refs/` (any Anki checkout), `local-data/` (the private collection and research notes).
- Anki, AnkiDroid, AnkiConnect or Migaku code, wherever it is: their repositories on the web, an installed Anki app (`/Applications/Anki.app`), Anki add-ons (`~/Library/Application Support/Anki2/addons21/`), Python packages `anki` and `aqt`, the macOS oracle venv (`build/oracle-macos-venv/`) (R1956).
- Similarity reviewer views and JPlag reports: they name or embed Anki sources. Shell commands on the main checkout's `build/mg/similarity*` and `build/similarity-index` are denied; read an implementer view the orchestrator points you to with the Read tool.

## Rules

- Write from the spec, in Meguri's own structure. Do not reproduce Anki code from memory, even when you think you remember it; the similarity scan (R1957) checks every change against every Anki release since 2.1.50.
- When a spec is unclear or incomplete, stop and ask for a spec update through the orchestrator. Never settle it by looking at Anki.
- Meguri authors its own stock note types, base card CSS, UI strings and documentation (R1959). Anki's exact wording is used only where a spec lists it (for example `specs/review/reviewer-strings.md`).
- No Anki web assets (reviewer scripts, image occlusion runtime, editor, MathJax configuration) in the app or in tests (R1960).
- UI work: at most three render-and-fix rounds per screen, then hand it to the critic and the gates (R1987).
- A similarity flag reaches you as the implementer view: file, lines, channel, score. Rewrite that region from the spec. Do not ask for the reviewer view. `tools/mg merge` copies it to `build/mg/merge-feedback/<run>/` in your worktree.
- Before handing back a round, run focused verification for changed behavior and affected builds. The filtered runner exports a patch; its orchestrator applies, commits and verifies through `tools/mg`, including provenance/privacy/similarity checks. Follow the full-gate cadence in `docs/process/tiers.md`. Nothing retries; a flaky test is fixed or quarantined, never rerun until green.

## Commits

Commits touching `core/`, `app/` or `conformance/runner/` carry these trailers; the commit-msg hook, which `tools/mg worktree new` installs, rejects them otherwise (`docs/process/provenance.md`):

```
Role: implementer
Spec: search/grammar#G3, search/grammar#G4
Session: s-20261013T091500Z-1a2b3c4d
```

UI work that no spec covers cites Product Contract requirement ids instead: `Spec: R1987, R150`.
