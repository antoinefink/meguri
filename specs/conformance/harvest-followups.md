# Harvest coverage owners

Spec: conformance/harvest-followups
Requirements: R1877-R1882, R1889, R1907, R1960
Anki: 26.09.3
Scenarios: conformance/catalog/harvest.csv

## H1. Classification and completion

The U4 supplement classifies the remaining 480 catalog rows from individual
behavior review at the pin. It leaves the earlier 699 rows unchanged. A
classification is a scope decision, not evidence that the behavior passes.
`deferred` retains a required coverage obligation under `conformance/dsl` D14.
It does not waive R1880 or permit a release with missing required coverage.

Each newly deferred row names its unfinished provider or assertion owner and
the behavior still to cover. U246 owns missing differential scenarios when an
accepted implementation already provides the behavior. U104 owns the final
Stats matrix and U91 the search result corpus. The named provider units own
their real operations; none may substitute fixture writes for those operations.
Any related existing scenarios must be checked for the whole behavior before
they are linked. Golden expectations come from oracle runs, never from the
harvested assertions. Native UI obligations require the corresponding native
tests as well as any engine observations.

## H2. Earlier deferred rows with incomplete owner tracking

This table adds ownership only. It changes neither the classification nor the
status, reason or scenario links of the baseline rows. An owner must add the
missing proof and then update the catalog. An already accepted provider is not
a reason to mark a row done.

| Harvest id | Unfinished owner | Coverage obligation |
|---|---|---|
| h0f4f7e722f1e | U107, U120, U246 | Incoming preset identity is retained only when the destination id is free; prove occupied-id handling. |
| h1df457b7a669 | U246 | Prove scheduling uses built-in defaults when both the requested and default presets are missing. |
| h3d8240dabf39 | U107, U120, U246 | Prove incoming legacy preset JSON is added through the real sync/import writer. |
| h3dc2e6c7647a | U246 | Prove a deck with a missing preset schedules using the default preset. |
| h69f9ba4c6483 | U107, U120, U246 | Prove the legacy incoming-preset path creates the required preset record. |
| h6a74f5728f8c | U246 | Prove lookup of a missing preset falls back to the stored default. |
| h774fd17fd8ea | U107, U120, U246 | Prove incoming preset ids are not reused over an occupied destination id. |
| hd3586154cc8d | U49, U246 | Add a real preset-removal undo scenario that restores the preset and affected assignment. |
| hd709478326d6 | U246 | Observe missing-id fallback through the real scheduling provider, not merely decoded defaults. |
| hf1fc2f580d06 | U107, U120, U246 | Prove free incoming preset ids survive the complete transfer. |
| haa85eb491ea1 | U232, U246 | Cover duplicate-name preset creation and its timestamp-derived unique name in the options flow. |
| h25a2a4a1f100 | U41, U246 | Add deck-tree count observations from the real queue/count provider. |
| h26a096646ed7 | U156, U195, U246 | Verify pickers omit the empty default deck where the picker contract requires it. |
| h34d675666d1c | U156, U195, U246 | Verify empty-default filtering in the second picker/listing path. |
| h5e505e6b3fb6 | U156, U195, U246 | Verify a picker that excludes filtered decks does not offer them as normal destinations. |
| h8673919e1db4 | U41, U246 | Prove deck-tree counts respect active daily limits. |
| hdddcc971bba4 | U41, U246 | Prove hierarchical deck counts through queue-backed tree observations. |
| h151bd9c1b80b | U48, U246 | Cover current/original/saved new position through real Reset and Set Due Date operations, including filtered home due. |
| h095feffe1089 | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h2a067e44c2ac | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h2dc9493eef0e | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h7f11536c09e7 | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| hb2bdcde89b3a | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| hf42c35381599 | U246, U66, U67 | Close the deferred display-transform assertion gap under H3. |
| h1f6151e74934 | U64 complete | Closed by format parsing plus actual IO div and stored-card review observations. |
| h3448f09e0c2c | U64 complete | Closed by the actual IO div observation, including attribute and class rules. |

## H3. Display proof and original graphics

The six display rows above still need an explicit assertion route. U246 owns
the per-row mapping to observable marker recognition, hidden-button behavior
and native action semantics. U66/U67 own the corresponding page and native
handler integration. The oracle's card HTML is not the final display transform,
so a render observation alone must not be called full coverage. The owner must
identify the exact assertion for each row before changing its status.

`render/display-transform` D4 and revised R526 define Meguri's original replay
graphic. Keep that independent design and R1907's approved replay-markup
normalization. No third-party graphic, CSS or JavaScript asset may enter a
scenario, expected snapshot or implementation as part of this follow-up
(R1960). The original graphic has its own explicit assertions; interoperability
checks retain the outer anchor, native action, numbering and visibility
contracts. This tracking document adds no comparison masking or allowlist.

## H4. Rows waiting on a new op, an observation or a native test

Session s-20261117T144450Z-f49841c9 classified these rows while covering others in the package and note type areas. They stay `deferred`. Each needs the op or observation named here, specified in the area's `specs/conformance/*-ops.md` together with its schema, testkit model and oracle driver (`conformance/AGENTS.md`), or the native test named here. The owner is the unit named in the row's catalog reason. Each sketch is the minimum that would make the obligation observable; the owner may choose another form that observes the same thing.

| Harvest id | Owner | Kind | Minimal addition and observation |
|---|---|---|---|
| h2071dec13b10 | U122, U246 | needs-op | `conformance/package-ops#E7` names are plain session file names, so no step can aim an export at a place that cannot be written. Add an optional key to E7's deck-package export, for example `dir: missing`, naming a session subdirectory that does not exist, and a closed rejection kind `destination_unwritable`. The step records `{export: {error: destination_unwritable}}`; a following `package_listing` of a name in the session root shows no file was written. On Meguri every failure the missing directory causes maps to that kind, whatever `package/apkg-export#A11` step finds it (preflight of the output's file system, the temporary file or publication), and leaves no artifact, staging directory or temporary file (A12). The user-facing half (no "Saved" when the native destination copy fails) is native: `package/file-export` E4 and E5. |
| h8ee8470f9917 | U120, U246 | needs-op | Every E6 import option is required, so no step reads the stored import options. Add an observation `import_defaults` (no argument) giving `{merge_notetypes, update_notes, update_notetypes, with_scheduling, with_deck_configs}` (`update_notes` and `update_notetypes` as `if_newer`, `always` or `never`). Oracle: Anki's own read of the deck-package import options, defaults included. Meguri: `package/apkg-import#I5`'s typed read of the five keys, defaults included (the pattern of `backup_settings` and `browser_config`). A scenario observes it on a fresh collection, then after `set_config` writes a value of another JSON type and then a valid value. |
| hb92ed5858653 | U120, U246 | needs-op | An E6 `name` that matches no session file is `unknown_ref` and a missing fixture is `invalid_scenario`, so a missing input cannot be recorded as a refusal. Add an E6.4 rejection kind `missing_file` for an import whose input file is absent (a `missing: <file name>` member, or `name` naming an absent file). Oracle: Anki's not-found error from its deck-package import. Meguri: the intake refusal of `package/file-intake#K3` step 3 and `package/apkg-import#I16`. The scenario observes `db_dump` and `media_dir` before and after the step: nothing changed. |
| h522f5887f377 | U120, U246 | not-observable | The import log half is shown by every E6 import step. The other half, the import response's change flags, is Anki's signal to its own UI about what to refresh; no DSL op records it, and Meguri reports changes through `engine/change-stream#S2.5` and the `package/apkg-import#I14.5` result. Close it with Meguri's own I14.5 and S2.5 tests, not with an observation. |
| hd276a7ef2889 | U131 | needs-op | `notes/empty-cards#E2` strips markup from the note type name and the joined card labels of the report (`text/strip-html#H4`); `conformance/empty-cards-ops#Y1` observes neither. Add an observation `empty_cards_labels` (no argument, after an `empty_cards` step): `[{notetype: <stored name>, label: <name as the report shows it>, notes: [{note: <ref>, labels: <joined card labels as shown>}]}]`. Oracle: the report text (a heading per note type, a label run per note). Meguri: U131's report data. A scenario renames a note type and a template to names holding a script element and checks that `label` and `labels` hold only the safe text. |
| h6288de9d6fb5 | U131 | needs-op | The same `empty_cards_labels` observation, with names that hold no markup: they reach the report unchanged. |
| h8c43ce71aafa | U131 | needs-op | The same observation, with an event-handler attribute and a script element in note type and template names: the markup is removed and the surrounding text stays. |
| h68d09bf06f67 | U37, U200 | needs-op | `notetypes/editor-actions#A2`'s list of note types with note counts has no observation (`notetype_structure` has no counts, and counting with `db_exec` would bypass the read under test, H1). Add an observation `notetype_list` (no argument): `[{name: <stored name>, notes: <count>}]` in A2's order. Oracle: Anki's names-and-counts read. Meguri: A2's ordered list and note counts joined by id. A scenario observes it, adds a note of one note type, and observes again: that count goes from 0 to 1 and the others stay. |
| h706cf39b03b8 | U37, U200 | needs-op | The same `notetype_list` observation: a note type with no notes is listed with count 0, the stock kinds of a fresh collection included. |
| hcd2cb9d07eec | U35 | native-ui | The Change Note Type sheet's pickers (`notetypes/editor-actions#A16`, U203): each new row lists the old names plus (Nothing); an old field may serve several rows, and in the template rows a choice another row uses is disabled (Anki instead moves the other row to (Nothing), a dialog rule the collection-library oracle cannot run). Native tests cover the rows, their choices and editing a row. The default maps the sheet starts from are shown by `notetypes/change/default-maps`. |
| he3cfa1e1e429 | U35 | native-ui | The unused-field list for a map the user has edited is computed by the sheet from one info result (`notetypes/change-notetype#M5`, M6; U203). Native tests recompute it after edits. The engine side is shown by `notetypes/change/default-maps` (`discarded_fields` for the default maps) and `notetypes/change/same-type-remap` (an unmapped old field's text is lost). |
