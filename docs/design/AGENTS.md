# docs/design/

- `screen-inventory.yaml` is the only screen inventory. `screens.md` is generated from it: edit the YAML, then `tools/mg design-docs --write`; `tools/mg design-docs` fails on a stale `screens.md` or an invalid entry. Status never goes in the inventory: it lives only in `screen-status.yaml` (U10, U254).
- `rubric.md` rule ids are cited by critic issues and decisions: never renumber or reuse one. Change a rule's meaning by adding a new id and retiring the old; bump the version and note it under Changes.
- `decisions.md` holds only Antoine's own picks and vetoes, appended; agents never add their own choices there.
- `references/`: Antoine captures the screenshots; agents never add, download or generate them. A new pattern id goes in `references/index.md` before the inventory cites it.
