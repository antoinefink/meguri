# Tag ops: tag management, flags and the tag tree

Spec: conformance/tag-ops
Requirements: R1883, R894, R895, R896, R897, R1176, R1177, R1178, R1190, R1191
Anki: 26.09.3
Scenarios: conformance/scenarios/tags/

The scenario ops (`dsl.md` D7.2) and the observation kind (`observations.md` O4) that the tags area needs. Both executors implement them with the meanings below; `conformance/schema/scenario.schema.json` and `observation.schema.json` hold their shapes. The behavior they run is `specs/tags/tags.md` and `specs/tags/flags-and-marked.md`. Tags and names are strings; a scenario quotes any value YAML would read as another type.

## G1. `add_note_tags`

- **Argument** `{notes: [#nN, ...], tags: <text>}`: at least one note; `tags` is the tag text exactly as a caller gives it, split by the executor (`tags/tags#T1`).
- **Effect** `tags/tags#T10` Add, as one operation.
- **Output** `{add_note_tags: {count}}`: the number of notes written.

## G2. `remove_note_tags`

- **Argument** `{notes: [#nN, ...], tags: <text>}`, as G1.
- **Effect** `tags/tags#T10` Remove, as one operation.
- **Output** `{remove_note_tags: {count}}`.

## G3. `delete_tags`

- **Argument** `{tags: <text>}`: the names to delete from every note and from the registry.
- **Effect** `tags/tags#T9`.
- **Output** `{delete_tags: {count}}`: the number of notes written (0 is possible while rows are removed).

## G4. `rename_tag`

- **Argument** `{from: <name>, to: <text>}`: `from` is one non-empty name without separators; `to` is the new name as typed (it may hold anything; T7 refuses or normalizes it).
- **Effect** `tags/tags#T7`.
- **Output** `{rename_tag: {count}}`.
- **Rejection** `{rename_tag: {error: "invalid_name"}}` for T7 step 1; nothing changes. Rejection kinds: `invalid_name`.

## G5. `reparent_tags`

- **Argument** `{tags: [<name>, ...], parent: <name>}`: at least one source, each non-empty; `parent` is the target name, or the empty string for the top level.
- **Effect** `tags/tags#T8`.
- **Output** `{reparent_tags: {count}}`.
- Scenarios avoid a source that T8 step 1 skips together with a remaining source while a note has a tag matching the skipped one: Anki fails with an internal error there, which poisons the oracle worker (`tags/tags#T8`, deviations).

## G6. `clear_unused_tags`

- **Argument** `{}`.
- **Effect** `tags/tags#T11`.
- **Output** `{clear_unused_tags: {count}}`: the number of rows removed.

## G7. `set_tag_collapsed`

- **Argument** `{tags: [<name>, ...], collapsed: <boolean>}`: at least one name.
- **Effect** `tags/tags#T12` with the names in order. The oracle runs Anki's set-collapsed operation once per name; Meguri runs one operation for the list. The rows they leave are the same, except where `tags/tags#T3`'s implicit-parent deviation spells a new row.
- **Output** none (`{}`).
- Scenarios give names that T2 leaves unchanged, and never set a row to the state it already has: both cases differ from Anki by design (`tags/tags#T12`).

## G8. `set_flag`

- **Argument** `{cards: [#cN, ...], flag: <integer ≥ 0>}`: at least one card.
- **Effect** `tags/flags-and-marked#F2`. No toggling: the toggle (F3) is the caller's rule.
- **Output** `{set_flag: {count}}`: the number of cards written.
- **Rejection** `{set_flag: {error: "invalid_flag"}}` for a flag above 7; nothing changes. Rejection kinds: `invalid_flag`.

## G9. `tag_tree` (observation kind)

- **Request** `tag_tree`, no arguments.
- **Value** the tree of `tags/tags#T13` as a list of top-level nodes in their order, each `{name, collapsed, children}`: `name` the component, `collapsed` a boolean, `children` a list of nodes. An empty registry gives `[]`.
- Nothing in the collection changes.

## G10. Counts and stamps

- Every op here except G7 and G9 outputs `{count}`. Anki counts the notes it matched for G3, G4 and G5, and Meguri the notes it wrote; they differ only for a matched note whose text does not change (`tags/tags#T6`). Scenarios avoid such notes.
- The ops read and write the collection like any user operation: the stamps are `tags/tags#T14` and `tags/flags-and-marked#F2`. Scenarios read them with `db_exec` and `db_dump` (`record-ops.md` W1, `observations.md` O8) after an `advance`, so that `now_s` differs from the creation time.

## G11. Status

- The oracle driver runs every op and the kind; the scenarios under `conformance/scenarios/tags/` have goldens.
- Meguri's executor implements them with U30's tag, flag and tree operations; until then the scenarios are `pending` (`dsl.md` D10). Until the testkit model (`core/crates/meguri-testkit/src/scenario/`) reads these ops and the kind, T0 reports the scenarios that use them as `invalid`; U30's implementer adds the typed arguments with the adapters.
