# docs/design/

- Use system SwiftUI styles by default. Change control styling only for a task or accessibility need; check the native result.

- Read `brief.md`, `workflow.md` and the active screen contract before UI work. Keep functional checks, user design agreement and final verification separate. HTML feedback does not approve native baselines.
- Mockups use one navigable entry point under `build/`; never put generated examples in `references/`. Confirm the exact source/product when reviewing a running native app.

- `screen-inventory.yaml` is the only screen inventory. `screens.md` is generated from it: edit the YAML, then `tools/mg design-docs --write`; `tools/mg design-docs` fails on a stale `screens.md` or an invalid entry. Status never goes in the inventory: it lives only in `screen-status.yaml` (U10, U254).
- `rubric.md` rule ids are cited by critic issues and decisions: never renumber or reuse one. Change a rule's meaning by adding a new id and retiring the old; bump the version and note it under Changes.
- `decisions.md` holds only Antoine's own picks and vetoes, appended; agents never add their own choices there.
- `references/`: Antoine captures the screenshots; agents never add, download or generate them. A new pattern id goes in `references/index.md` before the inventory cites it.
- `screen-status.yaml` is written only by `tools/scripts/screen-status/screen_status.py` (through `tools/mg critic` and `gallery`); `tools/mg design-docs` checks it lists every inventory screen. Sample screens (`sample.*`) never go in it: pass `--ledger` to a scratch file.
- `approved-baselines.json`, `reviews/` and pick lines in `decisions.md` come only from Antoine's gallery decisions (`tools/mg gallery serve`); agents never post decisions. An approval references the image by sha256 and git blob, so the approved image must be committed (the gallery refuses it otherwise). After an OS rebaseline the guard checks an approved entry through its successor (same model and cell, the new build); approving the successor retires the old entry.
- `critic-calibration/` examples come only from Antoine's decisions; `decisions/` holds spike records.
