# Spec format

Specs describe Anki's observable behavior that Meguri reproduces, in the spec writer's own words (R1954). Implementers build only from specs, scenarios and goldens, so a spec must be complete enough to implement without Anki.

## Files

- `specs/<area>/<topic>.md`, lowercase with hyphens. Areas: text, model, storage, engine, notes, notetypes, decks, tags, media, sched, render, search, browser, stats, sync, package, maintenance, servers, editor, review, deckconfig, conformance, fixtures.
- First lines:

  ```
  # <Title>

  Spec: <area>/<topic>
  Requirements: R1234, R1235
  Anki: 26.09.3
  Scenarios: conformance/scenarios/<area>/<dir>/
  ```

## Sections and spec ids

- Every normative section has an id in its heading: `## G3. Operator precedence`. An id is a capital letter prefix chosen per file and a number, with optional dotted sub-numbers: `G3`, `G3.1`. Ids are never reused or renumbered; a removed section keeps its heading with "Removed".
- A spec id is `<area>/<topic>#<id>`, for example `search/grammar#G3`. `<area>/<topic>` names a whole file.
- Commit `Spec:` trailers list spec ids, comma-separated. The paths `specs/search/grammar.md` and `specs/search/grammar.md#G3` are accepted too, and so are Product Contract requirement ids (`R1987`) for UI work that reproduces no Anki behavior. Implementer commits touching `core/`, `app/` or `conformance/runner/` must name at least one. Infrastructure units with no behavior to specify (U5–U7 and the tooling units) commit with `Role: infra` and no `Spec:` (`docs/process/provenance.md`).

## Allowed content

- Behavior stated in your own words, with examples and input-output tables.
- Exact constants, limits, defaults, formats and encodings.
- Names that are part of an interface: table and column names, protobuf fields, JSON and config keys, search keywords, sync endpoints, file names inside packages, error strings a client depends on.
- References to scenarios (`conformance/scenarios/...`) and to other spec ids.
- Known deviations from Anki, each pointing to its entry in `conformance/deviations.yaml`.

## Not allowed

- Anki code in any language, and pseudocode that follows Anki's control flow or structure.
- Anki identifiers (function, type, variable or module names) that are not part of an interface.
- Anki test code, fixture text or test names (R1881). The link from a scenario to the Anki test it came from lives only in `oracle/harvest/links.csv`.
- Anki file paths, line numbers or commit ids. Section-level provenance lives in `oracle/provenance/`.
- Anki web assets or stock note type templates and CSS (R1959, R1960).

## Interop vocabulary

`specs/<area>/interop-vocab.txt` lists the interface words of the area's specs, one per line, `#` for comments. Case and the separators `_`, `-`, `.` do not matter (`last_modified` also covers `lastModified`). The similarity scan drops these words from its `lex` channel, so reproducing Anki's interface names does not flag by itself. Add only interface names, never ordinary words.
