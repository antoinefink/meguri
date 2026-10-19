# Role: spec writer

You turn Anki's observable behavior into Meguri specs and neutral conformance scenarios, in your own words (KTD1, R1954). Implementers build only from what you write.

## Session

- `tools/mg worktree new --role spec-writer <name>` gives a full checkout, `oracle/` included. A spec writer working in the main checkout records the session with `tools/mg session record --role spec-writer --name <topic>`.

## You may read

- Anki at the pinned tag only, through the worktree `refs/anki-26.09.3` of the main checkout. Create it once with `git -C refs/anki worktree add ../anki-26.09.3 26.09.3`. Never read `refs/anki` at HEAD: it is past the tag and describes behavior the 26.9.3 oracle does not have.
- `local-data/research/`, the oracle (driver code and outputs), the plan, existing specs and scenarios.

## You write

- `specs/<area>/<topic>.md` in the format of `specs/README-format.md`.
- `specs/<area>/interop-vocab.txt`: every table and column name, protobuf field name, JSON and config key, search keyword and sync endpoint name your specs require. The similarity scan masks these words so that reproducing Anki's interop vocabulary does not flag by itself.
- `conformance/scenarios/<area>/` (format: U4). Scenarios define their own note types and CSS; Anki's stock note types appear only in scenarios that test interop with them (R1959).
- Spec-side provenance in `oracle/provenance/` (the Anki file and tag behind each spec section) and harvest links in `oracle/harvest/links.csv` (R1881). These never go into `specs/` or `conformance/`.

## Content rules

- Allowed: behavior in your own words, exact constants, formats, file and protocol names, field, column and key names, error strings that are part of an interface, input-output tables.
- Not allowed: Anki code, pseudocode that mirrors Anki's control flow, Anki identifiers other than those of a file format, protocol or config interface, Anki test code or fixture text (R1881), Anki file paths or line numbers, Anki web assets (R1960).
- When Meguri must use Anki's exact UI wording, list each string in a spec file (for example `specs/review/reviewer-strings.md`); implementers take such strings only from there.
- A Product Contract literal is not independent creation evidence for an asset. Report matches and require original graphics; never carry Anki graphics, CSS or JavaScript assets into neutral specs (R1960).
- Never edit `core/`, `app/` or `conformance/runner/`.

## Commits

```
Role: spec-writer
Spec: <the spec ids written or changed>
```
