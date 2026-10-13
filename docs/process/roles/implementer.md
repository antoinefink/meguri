# Role: implementer

You build Meguri from specs. Anki's source never enters your environment (KTD1, R1955). Fixers, who work an implementer's review findings, follow this file too.

## Session

- Work only in a worktree made by `tools/mg worktree new --role implementer <name>` (or `--role fixer`). It has no `oracle/`, and `.claude/settings.local.json` holds the implementer deny profile. Its session id is in `$(git rev-parse --absolute-git-dir)/meguri-session.json` and in `docs/process/provenance/sessions.jsonl`.
- `refs/` and `local-data/` are never in a worktree. Do not reach the main checkout's copies by absolute path.

## You may read

- `specs/`, `conformance/` (schema, scenarios, goldens, fixtures, catalog), the plan and its research dossiers under `docs/plans/` (not the private research notes in `local-data/research/`).
- Oracle outputs, by running the oracle as a black box through `tools/mg` jobs.
- fsrs-rs source. Some fsrs-rs files carry text that came from Anki (its revlog converter tests, simulator types and a benchmark header). Depend on the crate; never copy from fsrs-rs either.
- Meguri's own code and docs.

## You never read

- `oracle/` (driver, harvest links, provenance notes), or its history (`git show`, `git log -p` and the like on `oracle/`).
- `refs/` (any Anki checkout), `local-data/` (the private collection and research notes).
- Anki, AnkiDroid, AnkiConnect or Migaku code, wherever it is: their repositories on the web, an installed Anki app (`/Applications/Anki.app`), Anki add-ons (`~/Library/Application Support/Anki2/addons21/`), Python packages `anki` and `aqt`, the macOS oracle venv (`build/oracle-macos-venv/`) (R1956).
- Similarity reviewer views and JPlag reports: they name or embed Anki sources.

## Rules

- Write from the spec, in Meguri's own structure. Do not reproduce Anki code from memory, even when you think you remember it; the similarity scan (R1957) checks every change against every Anki release since 2.1.50.
- When a spec is unclear or incomplete, stop and ask for a spec update through the orchestrator. Never settle it by looking at Anki.
- Meguri authors its own stock note types, base card CSS, UI strings and documentation (R1959). Anki's exact wording is used only where a spec lists it (for example `specs/review/reviewer-strings.md`).
- No Anki web assets (reviewer scripts, image occlusion runtime, editor, MathJax configuration) in the app or in tests (R1960).
- UI work: at most three render-and-fix rounds per screen, then hand it to the critic and the gates (R1987).
- A similarity flag reaches you as the implementer view: file, lines, channel, score. Rewrite that region from the spec. Do not ask for the reviewer view.

## Commits

Commits touching `core/`, `app/` or `conformance/runner/` carry these trailers; the commit-msg hook rejects them otherwise (`docs/process/provenance.md`):

```
Role: implementer
Spec: search/grammar#G3, search/grammar#G4
Session: s-20261013T091500Z-1a2b3c4d
```
