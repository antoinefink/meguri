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
| h25a2a4a1f100 | U41, U246 | Add deck-tree count observations from the real queue/count provider. |
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

h26a096646ed7, h34d675666d1c, h5e505e6b3fb6 (deck choosers) and haa85eb491ea1 (preset names) moved to H4, which gives each its owners and obligation.

h25a2a4a1f100 is closed by `sched/counts/new-studied-today`, h8673919e1db4 by `sched/counts/nested-new-limits` for its limits obligation, and hdddcc971bba4 by `sched/counts/default-shown` (step 0: Default and Default::Sub both show 1 new card, read through `conformance/queue-ops#Q3`), all with recorded goldens. `default-shown` is already active; the other two become active once Meguri's `deck_counts` executor (U41) passes them. h8673919e1db4's harvested test also checks a parent node's own card total, with children not included. Meguri requires that value as `total_in_deck` of AnkiConnect `getDeckStats` (`servers/ankiconnect-actions#A5.8`), and `deck_counts` does not report it. The A20.1 transcript `servers/ankiconnect/decks/deck-stats` (a parent with 1 card and a child with 2) proves it. U136 and U142 own that transcript, and the row is not fully proven until Meguri passes it.

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

Harvest sessions classified these rows while covering others: s-20261117T144450Z-f49841c9 in the package and note type areas, s-20261117T152613Z-96466c16 in the deckconfig, decks, engine and package areas. They stay `deferred`. Each needs the op or observation named here, specified in the area's `specs/conformance/*-ops.md` together with its schema, testkit model and oracle driver (`conformance/AGENTS.md`), the `conformance/package-ops#E4.1` fixture named here, which the fixture maker builds, or the native test named here. The owner is the unit named in the row's catalog reason; where the reason names none, the units the row names, and where a row adds one, the row says why. Each sketch is the minimum that would make the obligation observable; the owner may choose another form that observes the same thing.

| Harvest id | Owner | Kind | Minimal addition and observation |
|---|---|---|---|
| h2071dec13b10 | U122, U246 | needs-op | `conformance/package-ops#E7` names are plain session file names, so no step can aim an export at a place that cannot be written. Add an optional key to E7's deck-package export, for example `dir: missing`, naming a session subdirectory that does not exist, and a closed rejection kind `destination_unwritable`. The step records `{export: {error: destination_unwritable}}`; a following `package_listing` of a name in the session root shows no file was written. On Meguri every failure the missing directory causes maps to that kind, whatever `package/apkg-export#A11` step finds it (preflight of the output's file system, the temporary file or publication), and leaves no artifact, staging directory or temporary file (A12). The user-facing half (no "Saved" when the native destination copy fails) is native: `package/file-export` E4 and E5. |
| h8ee8470f9917 | U120, U246 | needs-op | Every E6 import option is required, so no step reads the stored import options. Add an observation `import_defaults` (no argument) giving `{merge_notetypes, update_notes, update_notetypes, with_scheduling, with_deck_configs}` (`update_notes` and `update_notetypes` as `if_newer`, `always` or `never`). Oracle: Anki's own read of the deck-package import options, defaults included. Meguri: `package/apkg-import#I5`'s typed read of the five keys, defaults included (the pattern of `backup_settings` and `browser_config`). A scenario observes it on a fresh collection, then after `set_config` writes a value of another JSON type and then a valid value. |
| hb92ed5858653 | U120, U246 | needs-op | An E6 `name` that matches no session file is `unknown_ref` and a missing fixture is `invalid_scenario`, so a missing input cannot be recorded as a refusal. Add an E6.4 rejection kind `missing_file` for an import whose input file is absent (a `missing: <file name>` member, or `name` naming an absent file). Oracle: Anki's not-found error from its deck-package import. Meguri: the intake refusal of `package/file-intake#K3` step 3 and `package/apkg-import#I16`. The scenario observes `db_dump` and `media_dir` before and after the step: nothing changed. |
| h522f5887f377 | U120, U246 | not-observable | The import log half is shown by every E6 import step. The other half, the import response's change flags, is Anki's signal to its own UI about what to refresh; no DSL op records it, and Meguri reports changes through `engine/change-stream#S2.5` and the `package/apkg-import#I14.5` result. Close it with Meguri's own I14.5 and S2.5 tests, not with an observation. |
| hd276a7ef2889 | U131 | needs-op | `notes/empty-cards#E2` strips markup from the note type name and the joined card labels of the report (`text/strip-html#H4`); `conformance/empty-cards-ops#Y1` observes neither. The observation is now specified, planned, as `conformance/empty-cards-ops#Y5` (`empty_cards_labels`, no argument, after an `empty_cards` step; no schema, testkit model or driver yet): `[{notetype: <stored name>, label: <name stripped by H4>, notes: [{note: <ref>, labels: <joined labels stripped by H4, or null>}]}]`, with `labels` null for a cloze note type and when an empty ordinal has no template (R1887). Its planned scenario `maintenance/empty-cards/labels` (Y5.4) gives a note type name and a template name that each hold a script element, and checks that `label` and `labels` hold only the safe text. The row stays `deferred` until that scenario is recorded and U131 passes it. |
| h6288de9d6fb5 | U131 | needs-op | The same observation (`conformance/empty-cards-ops#Y5`) in the same planned scenario `maintenance/empty-cards/labels`: names that hold no markup reach the report unchanged. Deferred as above. |
| h8c43ce71aafa | U131 | needs-op | The same observation (`conformance/empty-cards-ops#Y5`) in the same planned scenario `maintenance/empty-cards/labels`: with an event-handler attribute and a script element in note type and template names, the markup is removed and the surrounding text stays. Deferred as above. |
| h68d09bf06f67 | U37, U200 | needs-op | `notetypes/editor-actions#A2`'s list of note types with note counts has no observation (`notetype_structure` has no counts, and counting with `db_exec` would bypass the read under test, H1). Add an observation `notetype_list` (no argument): `[{name: <stored name>, notes: <count>}]` in A2's order. Oracle: Anki's names-and-counts read. Meguri: A2's ordered list and note counts joined by id. A scenario observes it, adds a note of one note type, and observes again: that count goes from 0 to 1 and the others stay. |
| h706cf39b03b8 | U37, U200 | needs-op | The same `notetype_list` observation: a note type with no notes is listed with count 0, the stock kinds of a fresh collection included. |
| hcd2cb9d07eec | U35 | native-ui | The Change Note Type sheet's pickers (`notetypes/editor-actions#A16`, U203): each new row lists the old names plus (Nothing); an old field may serve several rows, and in the template rows a choice another row uses is disabled (Anki instead moves the other row to (Nothing), a dialog rule the collection-library oracle cannot run). Native tests cover the rows, their choices and editing a row. The default maps the sheet starts from are shown by `notetypes/change/default-maps`. |
| he3cfa1e1e429 | U35 | native-ui | The unused-field list for a map the user has edited is computed by the sheet from one info result (`notetypes/change-notetype#M5`, M6; U203). Native tests recompute it after edits. The engine side is shown by `notetypes/change/default-maps` (`discarded_fields` for the default maps) and `notetypes/change/same-type-remap` (an unmapped old field's text is lost). |
| h6d941db6b0ef | U124, U246 | needs-op | `conformance/package-ops#E9` has no rejections, so a collection package export with media that Anki refuses cannot be recorded. Add an E9 rejection kind `media_check_required`, recorded as `{export: {error: media_check_required}}`, the name E7 already uses. Oracle: the backend's check-media invalid-input error from the collection package export, told apart by its translation as in E7. Meguri: `package/file-export#E7.1`'s refusal of a folder name that fails `package/container#P6`, raised as `MediaNeedsCheck{name}` (`package/legacy-export#L11`). For this case E9's rule that scenarios give the media folder only names P6 accepts gives way. Use a name unsafe on every platform (such as `con.jpg`), not an NFD name: E7.1 and Anki on Apple both convert NFD to NFC. As in E7, the step's name is one no earlier step wrote, so after the refusal the session directory has no file of that name. |
| h1f2f223fd59f | U120, U122, U124, U246 | needs-fixture | The latest-format half is `package/collection-package/refused-media-map`. The legacy half, a legacy map name kept as `text/media-filenames#F1` transforms it (`package/container#P17`), needs a fixture, not an op: no Anki export writes such a name, and no op writes a raw package. Add to E4.1 `package/variants/legacy-media-reserved.colpkg`, a legacy 2 collection package (`collection.anki21` the raw bytes of `storage/schema-11.anki2`, the placeholder, entry `0` = the 3 bytes `abc`, `media` the JSON map `{"0": "con.jpg"}`). Scenario: `open` `storage/schema-18.anki2`, `timing`, import the fixture, `timing`, then `media_dir` holds `con_.jpg` (F1 step 4) with the SHA-1 of `abc`. U124 is added to the catalog's owners because the fixture is a collection package, whose import U124 owns. |
| h434af44650c7 | fixture maker, U120, U250, U246 | needs-fixture | Traversal (`../x.jpg`) and separator (`a/b.jpg`) names in imported media maps are refused as `corrupt` (`package/apkg-import/refusals`, `package/collection-package/refused-media-map`, `package/apkg-import/media-map-variants`); absolute names are shown only on export (`package/apkg-export/media-unsafe-references`). Build the `conformance/package-ops#E4.1` fixtures `package/variants/media-absolute.colpkg` and `package/apkg/legacy-absolute.apkg`, add one import step to each scenario E4.1 names for them, and record: each import is `{import: {error: corrupt}}` with `db_dump` and `media_dir` unchanged. |
| hfb585bb34348 | U232, U246 | native-ui | The picker's list, shown use counts and staged rename, add, select and remove are `sched/deck-options#O6.1`, O6.3 to O6.5; native tests cover them. Removing Default is refused by the save (`sched/options/remove-preset`, its first save: `default_preset`). O2's stored order and use counts are core snapshot output tested by Meguri's own tests (O13). |
| haa85eb491ea1 | U232, U246 | native-ui | The menu's duplicate-name suffix is `decks/deck-presets#P7` (O6.3, O6.4). The preset ops do not apply it (`conformance/deck-ops#K8`). Native tests and a core test of P7's helper with a clock frozen on both sides of a half second cover it. |
| h83f55b823be2 | U232, U246 | native-ui | The draft's save input (changed presets with the target last, the selected assignment, a staged-then-removed preset dropped, a stored one in removals) is `sched/deck-options#O6.3` to O6.5 and O7's input. `save_deck_options` cannot rename or stage presets. The removal's engine effects are in `sched/options/remove-preset` and `sched/options/remove-preset-undo-rows`; `sched/options/undo-save` shows the save as one undo step. |
| h34d675666d1c | U87, U156, U195, U246 | needs-op, native-ui | No observation reads a deck chooser's list (`deck_tree` and `deck_counts` follow `decks/decks#D12`, which shows the default deck when it has a child). Add an observation `change_deck_choices` (no argument) to `deck-ops.md`, with value a list of `{id, name}` (real id, full human name) in the chooser's order. Oracle: Anki's deck-names read with the empty default deck skipped and filtered decks left out. Meguri: the adapter calls U87's chooser-list read (`browser/bulk-actions#B7.1`, the read B17's engine tests cover), returns it unchanged, and does no filtering of its own. Scenario: a fresh collection with a normal deck and an empty `Default::<child>`. The default deck is left out and its child listed. After a note is added to the default deck, the default deck is listed. The native half: Go to Deck's copy of the rule (`decks/deck-list#H6.1`) is Swift-only, and U156's `GoToDeckFilterTests` cover it. U195's Add chooser follows D12 by design (`editor/add-flow#A2`). |
| h26a096646ed7 | U87, U156, U195, U246 | needs-op, native-ui | The same `change_deck_choices` observation. After a note is added to the default deck, the deck is listed. After that card is moved into a filtered deck inserted as K13 says (`odid` 1), the deck is left out again. The harvested read includes filtered decks, as Go to Deck does (H6.1): that half is native, in U156's `GoToDeckFilterTests` (H11 lists both cases). |
| h5e505e6b3fb6 | U87, U156, U195, U246 | needs-op, native-ui | The same `change_deck_choices` observation, with a normal deck and a filtered deck inserted as K13 says: only the normal deck is listed (B7.1). The other halves are native. Go to Deck lists the filtered deck (H6.1, U156 `GoToDeckFilterTests`). The Add chooser leaves it out (A2, U195 `AddScreenModelTests`). |

## H5. Optimizer rows held for the typed model

Session s-20261117T152613Z-96466c16 prepared and recorded scenarios for these rows. They cannot land yet: Meguri's typed scenario model does not parse `compute_fsrs_params` (`conformance/optimizer-ops#P1`, `conformance/dsl#D7.2`), so T0 and goldens-check would report the scenarios `invalid` (D10). The rows stay `deferred`.

| Harvest id | Unfinished owner | Coverage obligation |
|---|---|---|
| h14c25ad44110 | U47 | Add `compute_fsrs_params` to the typed model, then land `sched/optimizer/cutoff-learning-start` and `sched/optimizer/cutoff-splits-learning` with their goldens and link them. |
| hdf5b05a2842f | U47 | The same; land and link `sched/optimizer/cutoff-splits-learning`. |
| hccc87afa9021 | U47 | The same; land `sched/optimizer/training-item-counts` for the counts of `sched/fsrs-items#I7`. The item order and the aligned card ids are not observable on the oracle (`sched/optimizer#O17.1`, last bullet); U47's Meguri-only tests listed after O17.3's table cover them. |

## H6. Rows recommended for reclassification

Session s-20261117T152613Z-96466c16 found that these rows test a count that no Meguri behavior has and no op reaches. They stay `deferred` in the catalog. The table records a recommendation: the owners named in the catalog reason reclassify each row as `not-applicable` or `out-of-scope`, a scope decision (H1), and update the catalog.

| Harvest id | Owner | Recommendation and reason |
|---|---|---|
| h1586af627542 | U95, U104 | Reclassify. Anki's per-deck studied count (rated review-log rows of kind learning, review or relearning, or of another kind with a nonzero factor, counted as distinct cards per home deck, strictly after the start of the day) feeds only its pass that reschedules cards when FSRS options change, as today's workload per preset. That pass runs only when rescheduling on change is requested and the load balancer is enabled. No op runs it: every deck options, FSRS policy, optimizer and timed-operation driver op sends `fsrs_reschedule` false. Meguri never reschedules on change (R697, R752, `sched/memory-state#M4.1`, and the "reschedule on change" row of `sched/optimizer#O16`: Meguri has no such option), so it has no aggregate with this rule. |
| h5a31d6105048 | U95, U104 | Reclassify, for the same reason: the same per-deck count (home-deck attribution, unrated rows left out). Meguri's home-deck rule for review rows is the heatmap deck scope, `stats/counting-rules#N4`, with its own scenarios. |
| hb30e802d8f2f | U95, U104 | Reclassify, for the same reason: the same per-deck count (strict day-start boundary). The same boundary for the Home studied-today numbers is shown by `storage/revlog/studied-today-window` and `stats/today/day-start-utc`. |

## H7. Further rows held by session s-20261117T152613Z-96466c16

Session s-20261117T152613Z-96466c16 reviewed these rows and they stay `deferred`. The kinds, the owner rule and the meaning of a sketch are those of H4. `blocked` means a recorded scenario exists but cannot land yet; it is named in the row, it is not linked in the catalog, its scenario and golden are kept on the branch `held/harvest-batch-2` (every scenario that uses `change_deck`, which the testkit does not parse yet; re-record them when they land), and the row becomes `covered-new` only in the change that unblocks it. Changed 2026-11-21 (implementation review): the scenarios that use `find_replace` or `find_replace_fields` landed with U86 and are active (`browser/find-replace` F12); the `blocked` rows that name only such scenarios are unblocked, and they become `covered-new` when their catalog rows are linked to those scenarios. Changed 2026-11-21 (implementation review): ten of them are linked and `done` in the catalog, after a check of each against its test at the pin: hf7c80804a0ae, h6b3ffb448b14, h8b9aff97d516, h3bc595fa00b0, h6d2b664432d7, h22d21da07c9d, h8838c90310d2, h0b434e5e6376, h102f0046f2fd and hcd7b5a34f3ac, to the scenarios their rows name. h28ba189d8c7e stays `deferred` (its row below). A row that closes on native tests also needs the page or editor golden it names, where it names one (H1). This session's rows in H4 (deck presets, deck choosers, collection packages), H5 (optimizer) and H6 (reclassification) are not repeated here. Rows already listed in H2 or H3 keep the obligations there; the entries below add the route to close them.

### H7.1 Browser and search

| Harvest id | Owner | Kind | Minimal addition and observation |
|---|---|---|---|
| hf7c80804a0ae | U86, U246 | blocked | Prepared scenario `browser/find-replace/mixed-notetypes` (recorded golden). A regex `find_replace` with an inline case flag over five notes of three own note types writes only the three matching notes and returns count 3; `find_replace_fields` gives the sorted union of field names, a shared name once, and the Vocab-only list; a literal run limited to field `Text` skips notes without that field and returns count 2. Held until U86 adds `find_replace` and `find_replace_fields` to the testkit scenario model (`conformance/bulk-ops#X5`); `goldens-check` reports it invalid until then. |
| h6b3ffb448b14 | U86, U246 | blocked | Prepared scenario `browser/find-replace/literal-dollar-replacement` (recorded golden): in literal mode `$3` and `${1}$$x$name` are stored exactly as typed, while regex mode expands the capture. Held for the same testkit work as hf7c80804a0ae. |
| h8b9aff97d516 | U86, U246 | blocked | Prepared scenario `browser/find-replace/whole-collection-count` (recorded golden): an all-notes run gives count 3, a run limited to field `Back` gives 1, and a run with no match gives 0 and writes nothing, `col.mod` included (`browser/find-replace` F6, F8, F9). Held for the same testkit work as hf7c80804a0ae. |
| h0d07ce6ed51b | U84, U246 | needs-op | No observation reads the column catalog; row reads take their keys from the scenario. Add an observation `browser_columns` (no argument): `[{key, sort_cards, sort_notes, alignment, uses_cell_font}]` sorted by key (sort values `none`, `ascending`, `descending`; no labels). Oracle: Anki's catalog read, sorted by key because its own order depends on the locale. Meguri: the `browser/columns` C2 catalog with `browser/config-keys` K3's sortability. A scenario observes exactly the 19 keys on a fresh collection. |
| h04f566130b07 | U88, U246 | needs-op | Both sides refuse a search group with no items (`search/join-and-replace` J3), but `conformance/search-ops` S5 and the schema forbid such groups and S6 treats `invalid_item` as a run error. Let `search_build` accept `{group: {joiner, items: []}}` at the top level, as a member and under `negated`, with output `{error: invalid_item}`; change S5, the schema's non-empty rule and S6 in the same change. Oracle: Anki's invalid-input refusal mapped to `invalid_item`, never a reason read from its message. |
| h2e49ab2ea87a | U88, U246 | needs-op | The same `search_build` change as h04f566130b07; the scenario also shows the refusal is never `deck:*` or empty text. Until then U88's builder tests cover J3. |
| he244355f799a | U82, U91 | blocked | Prepared scenario `search/state/memory-state-moves` (recorded golden), held until the testkit scenario model parses `change_deck`; land it with that variant or with the held `notes/change-deck` scenarios. It shows the selection is "stability absent", which `sched/memory-state` M1 does not yet state (spec gap proposed). It does not prove the "new cards left out" half, which has no visible effect through Meguri's only consumer. |

### H7.2 Deck options

| Harvest id | Owner | Kind | Minimal addition and observation |
|---|---|---|---|
| heaf6b0314ef9 | U49, U246 | needs-op | No observation exposes the options read. Add a read-only observation `deck_options_read {deck}`: `{current_deck: {name, config_id}, presets: [{name, use_count}], limits: {new, review, new_today, review_today, new_today_active, review_today_active}, new_cards_ignore_review_limit, apply_all_parent_limits}`. Oracle: Anki's options read for the deck, mapped to names. Meguri: `sched/deck-options` O2 items 1 to 4 projected to those members. A scenario must not observe a deck whose preset is missing (O12), or it needs a deviation. |
| h1c1a2a378925 | U49, U246 | needs-op | The recorded scenario `sched/options/save-writes` proves every part except a preset staged inside the save: E1 can name only presets that already exist. Add an optional E1 member `add: [{name, options?}]` (`sched/deck-options` O6.3, O7.2 step 2) with `preset` allowed to name one of them. Observe `deck_config` rows (a new id other than 0 and 1) and the deck's `conf` equal to that id. Oracle: a preset with id 0 in the update request. Meguri: U49's staged add. |
| h1e72f458212b | U234, U246 | needs-op | Step text exists only in the options screen. Use the draft observation `step_text` (`conformance/deck-options-ops` E6), list form, format and parse entries. Oracle: the pinned build's options page run offscreen, as for `stats_presentation` (`conformance/stats-ops` H14). Meguri: U49's `sched/deck-options` O5 functions. Planned scenario `sched/options/step-text-units`: format 2, 15, 180, 240 and 2880 minutes to `2m 15m 3h 4h 2d` and parse it back; no value a multiple of 43,800 minutes (O12). |
| h35f01eb07a7f | U234, U246 | needs-op | The same `step_text` observation. Planned scenario `sched/options/step-text-seconds`: format the f32 values of 2/60, 20/60, 0.75, 2.25 and 75 minutes to `2s 20s 45s 135s 75m` and parse it back, compared as f32 (O5.1, O5.2). 2.25 minutes rules out an implementation that uses `m` whenever the step is at least a minute. |
| h1dc1c50d96e2 | U234, U246 | needs-op | The same observation, format side only (O5.2 rules 1 to 3). Planned scenario `sched/options/step-text-rounding`: the stored f32 of 7/60 formats to `7s` and that of 40/60 to `40s`. Reading `legacy_json` shows only the stored f32, not the text. |
| h90b32cd0020c | U234, U246 | needs-op | The same observation, parse side (O5.1). Planned scenario `sched/options/step-text-empty`: empty, space-only and tab-only text each give `{steps: []}`; parse scenarios separate tokens with single spaces only (proposed O12 amendment). Bad tokens cannot be oracle-proven: Meguri's `step_malformed` is covered by its own O4 tests (O13). |

### H7.3 Editor formatting

The oracle drives no editor page (`editor/formatting` F17), so each row needs both its F17 page golden recorded from Anki and U192's native test compared with it (H1, F4 **Goldens**). Rules are `editor/formatting` F2 (selection afterwards) and F4 (applying, removing, command scope).

| Harvest id | Owner | Kind | Rule and F17 rows |
|---|---|---|---|
| h0c55254c24f2 | U192, U246 | native-ui | F4 Applying step 1 and the `a[b]c` row; F2. Row `bold-text-suffix`. |
| ha80fc2c2d74d | U192, U246 | native-ui | F4 Applying step 1; F2. Row `bold-prefix`, with a typed character to show the selection. |
| hedaf87a0aa38 | U192, U246 | native-ui | F4 Applying steps 3 and 4; F2. Row `bold-whole-run-selection`. |
| h84ceecde6896 | U192, U246 | native-ui | F4 step 5 (merge into the `u` before, bold kept). Rows `underline-merge-nested-after`, `underline-merge-nested-before`. |
| hd739e449be5d | U192, U246 | native-ui | F4 step 5 (merge into the `u` after, bold kept). Row `underline-merge-nested-before`. |
| h08dfde967274 | U192, U246 | native-ui | F4 step 5 and the command scope (italic inside the neighbour kept). Row `bold-merge-keeps-italic`. |
| h859f07d44ca0 | U192, U246 | native-ui | F4 step 5 and the command scope (redundant nested bold flattened). Rows `bold-merge-flatten`, `bold-flatten-scope`. |
| hd9206f0a281f | U192, U246 | native-ui | F4 step 5 and the command scope (nested bold in the merged neighbour flattened). Row `bold-flatten-merged-neighbour`. |
| h7ab84f3f0f1b | U192, U246 | native-ui | F4 step 6 (extending over a neighbour whose whole text is bold). Row `bold-extend-before-nested`. |
| h2c5d0a1cf58b | U192, U246 | native-ui | F4 step 6, mirror case. Row `bold-extend-after-nested`. |
| ha59b9ffb5db2 | U192, U246 | native-ui | F4 steps 6 and 7 (extending over two neighbours, both non-bold wrappers kept). Row `bold-extend-nested-neighbours`. |
| h13b7128734ea | U192, U246 | native-ui | F4 step 3 (no wrapper crosses a block; void elements at a run's edge stay outside; a run with no text gets no wrapper). Rows `bold-br-before-list`, `bold-merge-across-break`, `bold-image-only`, `bold-text-then-image`. |
| h6f0c84dadbdb | U192, U246 | native-ui | F4 step 3 (a `br` between content of one run is wrapped with it); F2. Row `bold-br-run`, selection probed. |
| hb640064a2bb5 | U192, U246 | native-ui | F4 step 5 (merging passes over a `br`). Row `bold-merge-over-break`. |
| he1ecc4d09556 | U192, U246 | native-ui | F4 step 5 with boundary points inside the text node; F2 (the merged neighbour is not selected). Row `bold-merge-over-break-in-text`, with a typed character. |
| hb93c4bd5833b | U192, U246 | native-ui | F4 toggle decision and steps 2 and 3 (flat bold, trailing `br` outside). Row `bold-flatten-selected`. |
| hc76586f0804d | U192, U246 | native-ui | F4 steps 2 to 4; F2 (every italic element stays separate). Row `bold-across-italic-runs`, with a typed character. |
| h7508907b7b47 | U192, U246 | native-ui | F4 toggle decision, Removing, and step 3 (bold outside the selection rebuilt per block; edge `br` outside). Rows `unbold-list-item-in-bold`, `unbold-break-at-edge`. |
| hd919a62a1c8a | U192, U246 | native-ui | F3 (unwrap) and F4 Removing, the `<b>[abc]</b>` row. Rows `bold-over-strong` (its second Bold) and `unbold-between-text`. |
| hd9775245329a | U192, U246 | native-ui | F4 toggle decision (selection inside the element, or the command applies instead), F3, F4 Removing. Row `unbold-between-text`. |
| hef00e615d756 | U192, U246 | native-ui | F4 toggle decision and Removing (unselected nested bold kept in its own wrapper); F2. Row `unbold-nested-partial`, selection probed. |

### H7.4 Editor fields, paste, media and Add

The oracle runs no editor page or Add screen (`editor/paste-filter` X10, `editor/paste` P1). Where a row names a `editor/paste` P24 golden, that editor-page golden is also required once the editor-page oracle exists.

| Harvest id | Owner | Kind | Native test and rule |
|---|---|---|---|
| h1b75a8055e30 | U190, U246 | native-ui | U190's commit-timer tests on `editor/field-html` S1: an input during a flush without `hold` restarts the idle timer, the end of the flush does not cancel it, and the next commit carries the latest content (proposed S1 and T2 additions). |
| hdf1ad6d4cdf4 | U197, U246 | native-ui | `PasteFilterConformanceTests` on `editor/paste-filter` X6, X7 and `editor/paste` P9 step 2, P16: empty HTML in every mode inserts nothing. Needs the proposed X9 empty-input row. |
| h0fbabf88d405 | U197, U246 | native-ui | `PasteFilterConformanceTests` on X7 and P15.3, with X9's internal-content rows (sizing and image alignment removed; weight and text alignment kept; an emptied style becomes `style=""`). |
| h3a731837bd25 | U197, U246 | native-ui | `PasteFilterConformanceTests` on X4 (unknown elements unwrapped, empty ones removed), X1 and P15.1, with X9's nested unknown-element row. |
| ha1793751ed20 | U197, U246 | native-ui | `PasteFilterConformanceTests` on X3, X4, P14.1 and P15.1: script removed with its content, top level and nested. Needs the proposed X9 nested-script row. |
| h1ca43e6f5d18 | U197, U246 | native-ui | `PasteFilterConformanceTests` on X2 and X4 (`title` removed with its text, inherited by X5), P15.1, P15.2, with X9's mixed title, script and style row in strip, keep light and keep dark. |
| hb014ea4e2147 | U197, U246 | native-ui | `PasteFilterConformanceTests` on X4 and X5, P15.1, P15.2: strip unwraps `b` and `a`; keep keeps them, and `a` keeps only `href`. |
| ha86f3657ab34 | U197, U246 | native-ui | `PasteFilterConformanceTests` with the editor in dark appearance, on X5.1 and P15.2, with X9's keep-dark rows (colors removed, emphasis styles kept). |
| hdf20698825af | U197, U246 | native-ui | `PasteFilterConformanceTests` on X4 and P15.1 (no attributes on blocks; `img` keeps `src` and `alt`), with the contrast that internal content keeps handlers (X7). |
| he7d1b49afb31 | U197, U246 | native-ui | `PasteFilterConformanceTests` on X4, X5.1 and X7: keep mode in light appearance removes only a transparent background color; internal content keeps it. P24 `keep-filter`, `strip-filter`, `internal-paste`. |
| h1d03e06c9c80 | U197, U246 | native-ui | The proposed `editor/paste-filter` X10.1 paste-then-save cases 1 to 3: a flag the payload would set stays unset, and neither the field DOM nor the stored field holds the script. P24 `strip-filter`, `keep-filter`. |
| h2e0dbc5abe92 | U197, U246 | native-ui | X10.1 cases 2 and 3: no script element and none of its text in the field DOM or the stored field (P14.1, X3, X4). |
| h32d716cc068a | U197, U246 | native-ui | X10.1 cases 1 to 3: no handler runs, no `on*` attribute remains, text and `img` (with `src` and `alt` only) stay (P14, P15.1, P15.2, X4, X5). |
| hc0e055822e0f | U197, U246 | native-ui | X10.1 cases 2 and 3: a pasted `p` becomes a `div` with no attributes in the field DOM and the stored field (P15.1, P16, X3). |
| h4d2c40a0930e | U194, U197, U246 | native-ui | U197's `PasteProcessorTests` on `editor/paste` P7 and P11 with both values of the PNG-preference key (`editor/config-keys` K2, `editor/paste-sources` S8). P24 `image-encode`. Engine evidence only: `editor/paste/store/image-names`. |
| hd319020c121d | U194, U197, U246 | native-ui | U197's `PasteProcessorTests` and `PlainTextToHTMLTests` on P13 and P14.2 (decoded bytes, `paste-<sha1>.png`, src rewritten), S10. P24 `text-keep-tokens`, `html-img-sources`. Engine evidence only: `editor/paste/store/image-names`. |
| h88ee1a8ec135 | U194, U246 | native-ui | U194's snippet tests on P3: a name that is neither image nor audio or video is inserted as plain text and nothing plays. P24 `snippets`. |
| hbed095c14b47 | U194, U246 | native-ui | U194's snippet and playback tests on P3, P12 and S9: an audio name gives `[sound:NAME]` and plays once. P24 `snippets`. Engine evidence only: `editor/paste/store/snippet-save`. |
| hc53d1db14186 | U194, U246 | native-ui | U194's extension-classification test on P2 and P3, with image names and unrelated suffixes as negative cases. |
| hcddb2db9708d | U194, U246 | native-ui | The same classification test with mixed-case inputs for each of P2's 21 audio and video extensions. |
| h0af129b47980 | U194, U246 | native-ui | The proposed P25 Record round trip: the recording goes into the field recorded when Record started (P20, P25). |
| h9de5cb7af907 | U194, U246 | native-ui | The proposed P25 Record case: the take is added as a `[sound:]` snippet at the caret of that field and played once (P20, P3). |
| h5fce9ef76c1f | U194, U246 | native-ui | The proposed P25 Attach round trip (P4, P5, P3). P24 `snippets` for the snippet form. |
| hd717744a70ee | U194, U246 | native-ui | The proposed P25 cases: content arriving after focus moves goes only into the recorded field, and nothing is inserted after the note changes (P25 steps 1 to 4). |
| h601407f24351 | U205, U206, U246 | native-ui | The proposed `IOAddEditUITests` Add-mode case: draw a mask, switch at once to the fields, type in Header, Add; the note's Occlusion field holds the new masks (`editor/mask-editor` M9, `editor/image-occlusion-notes` O6.1). |
| h5e551aa48cb4 | U195, U246 | native-ui | `AddScreenModelTests` and `AddFlowUITests` on `editor/add-flow` A2 and A5 step 5: the add uses the chooser's deck. Engine evidence only: `editor/add-flow/notetype-deck-memory`. |
| h92ef3cd2c306 | U190, U195, U246 | native-ui | `NotetypeSwitchMapperTests` and `AddScreenModelTests` on A3 steps 2 and 5 and A3.1. |
| h3632fc2d2bac | U190, U195, U246 | native-ui | The proposed, more exact A11 line in `AddFlowUITests`: two fields typed, focus left in the second, Add within 100 ms, both values stored (A5 step 2, `editor/field-html` S1). |
| hd760eb8143a3 | U190, U195, U246 | native-ui | The proposed A11 assertion: an Add-screen add sends no note-update op and adds only the "Add Note" step; plus the next draft (`notes/add-defaults` D6, D7; A5 step 6). Engine evidence only: `editor/add-flow/add-leaves-other-notes`. |
| hd9a760a2f259 | U195, U246 | native-ui | A11's refusal case in `AddFlowUITests`: no add, the refusal line shown, focus in the first field (A5 step 4, `notes/field-check` K5). The engine half is in `notes/fields-check/states` and `editor/add-flow/add-leaves-other-notes`. |
| h8c899c6506a2 | U190, U195, U246 | native-ui | `AddFlowUITests` on A4: the pin shows the stored sticky option and follows each change. The persistence half is recorded in `editor/add-flow/sticky-pin-each-way` and `editor/add-flow/sticky-toggle`. |
| hc01768203874 | U195, U246 | native-ui | `AddScreenModelTests` and `AddFlowUITests` on `notes/add-defaults` D6 and A5 step 6: sticky values kept, other fields cleared. The engine half is recorded in `editor/add-flow/sticky-pin-each-way`. |

### H7.5 Engine, text and sync errors

| Harvest id | Owner | Kind | Minimal addition and observation |
|---|---|---|---|
| h69f4adbd5970 | U250, U246 | needs-op | The reopen half is shown by reopen scenarios such as `preservation/churn/relaunch`. Add a create form to `conformance/storage-ops` P1, `open: {create: <session file name>, place: missing_dir \| read_only_dir \| unreadable_file}`, a rejection kind `cannot_open`, and a session-directory listing to show nothing was made. Oracle: Anki's open refusing; Meguri: `storage/open-and-connections` C2 failing with `Io`. A worker running as root ignores permission modes, so the driver runs as an ordinary user or uses only `missing_dir`. |
| h46a2e2a48929 | U250, U246 | not-observable | Scenario names resolve against the session directory, so no op takes a process-relative destination. Close with a `meguri-storage` test under `storage/durability-and-files` F4 and F4.1: from a temporary current directory, publish to a bare file name; the file exists there and the directory sync ran on that directory. |
| h7e4c18df6974 | U167, U246 | native-ui | Interface text is never observed. Native string tests follow `render/display-transform` D8, D10 and D12 for the error notice (Meguri's own wording), and `review/reviewer-strings` S1 with `text/interface-text` I3 and I8 for one and other plurals. `render/errors/stored` already checks the error kind and side. |
| h420d96663fe4 | U167, U246 | native-ui | Native catalog tests under `text/interface-text` I8 for English fallback, argument substitution and plurals (I1 to I3, S1); the proposed I8 case (1) adds fallback and substitution. |
| h74b7d2e31f18 | U167, U246 | native-ui | Native formatter tests under I8 on I6: de-DE writes a comma and en-US a full stop even in an English interface, and the I4 core texts keep a full stop. |
| hbcc38ba4659c | U167, U246 | native-ui | U178's `AnswerButtonsTests` (`review/reviewer-strings` S12, S13) and I6.1: the plural category of a quantity with a fraction follows the written, rounded number. |
| h3338e90d7270 | U106, U246 | not-observable | The oracle cannot inject chosen diagnostic text, and Meguri's user message is its own per kind. The `sync/errors` E11 table test (a whitespace-only detail becomes absent) and, where a U151 or U160 surface shows diagnostics, a native check under the proposed E1.1. Frozen with U106 until the server work resumes. |
| hea8cbd8d6720 | U106, U246 | not-observable | The same tests for a generic network failure with an empty detail. Frozen with U106. |
| h8e35ff39a69f | U106, U246 | not-observable | E11's timeout row (stall watchdog through the scripted stub, `sync/transport` T8, T13) asserts an absent detail, plus the E1.1 check. Frozen with U106. |
| h56ce4672d339 | U106, U246 | not-observable | E11: a non-blank detail survives byte for byte apart from E1's redactions, and a U151 or U160 diagnostics surface shows it. Frozen with U106. |

### H7.6 Notes

| Harvest id | Owner | Kind | Minimal addition and observation |
|---|---|---|---|
| h7f3ad7c57016 | U20 | not-observable | Created GUIDs are random and no op exposes the encoder. U20's Rust tests check `text/guid` G2 against G4's table. |
| ha4fba0e6523a | U137 (U136, U246) | blocked | Waits on the AnkiConnect transcripts of `servers/api-undo-history` Y4 (update-note-fields, update-note, update-note-tags-only, update-note-fields-unchanged), still to be recorded; U136 and U246 own the recording. An optional `edit_note` key `undoable: false` (`conformance/record-ops` W2) would add little: Meguri differs on purpose (Y3, `engine/undo` M7), so each Meguri run would need a deviation. |
| hf580907ab941 | U35 | needs-op | `browser/bulk/change-notetype-mixed`, `notetypes/change/refusals` and `notetypes/change/default-maps` exercise `notetypes/change-notetype` M4, but the oracle driver restates the rule rather than calling Anki's single-note-type request. Oracle-maintainer task under `conformance/change-notetype-ops` H3: resolve the old note type with that request, map its not-found and several-note-types errors to `not_found` and `multiple_notetypes`, and re-record the three scenarios. |
| h83c15fb8bf40 | U86 | needs-op | Add an op `refresh_notes {notes, add_cards, stamp}` -> `{count}`, rejection `field_count`. Oracle: Anki's after-note-updates request; Meguri: U86's refresh. Scenario: `db_exec` creates the outside change (stale sort field and checksum, unnormalized text, an unregistered tag, a field that newly makes a template non-empty), then the op runs with both flags false and both true; observe notes, tags, `col.mod` and cards per note, with a deleted note skipped and a field-count mismatch refused. Needs the owner decision of the `notes/note-records` U4 spec gap. |
| h9c660fb50a5e | U120 | needs-op | Add an op `add_notes {notes: [<add_note argument>, ...]}` -> `{notes, cards}` in list order (`notes/note-records` A5). Oracle: Anki's add-notes request; Meguri: A5's bulk add. Scenario: three notes of two note types into two decks; due positions ascend in list order, one undo step "Add Note", and one undo removes all three with no graves. |

### H7.7 Rendering and descriptions

The six display rows are also in H2 and H3; these are the assertions H3 asks for.

| Harvest id | Owner | Kind | Native test and rule |
|---|---|---|---|
| hb2bdcde89b3a | U62 | native-ui | Display tests on `render/display-transform` D6 (hidden play buttons remove every marker D5 does not claim) and D3; D12 snapshots. |
| h7f11536c09e7 | U62 | native-ui | D6 and D3: text without markers is unchanged. |
| h2dc9493eef0e | U62 | native-ui | D6 with D3 (one left-to-right pass) and D9: markers from both sides are removed; D12's "hidden buttons with the AV lists unchanged" snapshot. |
| h2a067e44c2ac | U62 | native-ui | D3 and D4: `play:q:N` recognized and carried by the replay button. Marker production is shown by `render/av-tags/numbering`. |
| hf42c35381599 | U62 | native-ui | D3 and D4: `play:a:N` written exactly as in the marker. |
| h095feffe1089 | U62 | native-ui | D3: a raw `[sound:...]` tag is not a marker; D11 records Anki's looser pattern. |
| hf55fc72cd71b | U67 | native-ui | U67's player tests on `render/audio-playback` A5 and `render/av-tags` A6: a sound entry plays its safe base name from the media folder. |
| hebddab91941f | U155, U246 | not-observable | `text/description-html` has no oracle parity (D2, D3). Meguri unit tests from D4, with the proposed Markdown heading row, and U155's deck-page test. |
| h82c4a097602a | U155, U246 | not-observable | The same, with the proposed D4 row where `md` is true and a raw script block follows text (D3.1 removes it with its content). |

### H7.8 Scheduling and statistics

| Harvest id | Owner | Kind | Minimal addition and observation |
|---|---|---|---|
| h319667c38026 | U44, U246 | needs-op | No observation reads a state's leech status (`sched/leeches` L5, `sched/answering` A13). Either the answer op outputs `{leech, suspended}`, or a new observation `leech_states(#cN)` gives the leech status of each choice. Scenario: threshold 8, no relearning steps; Again at 7 lapses is leeched, at 9 not, Good not. |
| h837dc023b887 | U44, U246 | needs-op | Add an op `update_deck_stats {deck, new_delta, review_delta, ms_delta}`, no output. Observe `legacy_json` of the deck and a parent and lowered `deck_counts`; a later day shows the reset. The spec rule is missing too (spec gap proposed). |
| h33a369b7b2d4 | U92, U104, U222 | native-ui | U92's core test (`stats/card-info` C14.2) on C4.1 rows 7 to 9 and C4.3, and U222's display test. Raw inputs: `stats/card-info/memory-and-retention` (#c1) and `stats/card-info/ae46-no-state`, both with no stored retention. |
| h40284ca51ce0 | U92, U104, U222 | native-ui | The same tests: retention alone gives no Stability or Difficulty row and no Ease row. Raw input: `memory-and-retention` #c3. |
| hf27c621cefe8 | U92, U104, U222 | native-ui | The same tests: difficulty as a whole percent (C4.1, C5.4), no Ease row. Raw inputs: `memory-and-retention` #c2 and `stats/card-info/review-card`. |
| h7d47d968bfb6 | U226 | native-ui | Tests of `sched/deck-options` O5.2: the largest of d, h and m that divides exactly, otherwise s (61 s, 3,600 s, 86,399 s); O12 records the whole-month case. |
| hd903a0575683 | U226 | native-ui | Pure-function tests of `stats/graph-presentation` P2.2 step 1 (seconds, minutes, hours, days, months, years thresholds). |

### H7.9 Rows proven only by held scenarios

These rows have recorded scenarios that prove them, but the scenarios cannot land yet, so they are kept on the branch `held/harvest-batch-2`. Either they use `find_replace`, `find_replace_fields` or `change_deck`, which the testkit does not parse yet, or they sit in a directory whose core test lists every scenario it runs and requires each to pass (the sched `ops`, `answer`, `leech`, `counts`, `queue`, `session` and `steps` next-states families, `render/type-answer`) or under an `active` pattern of `area.yaml` while still pending (`render/filters/kana-stored-edits`). A spec table marks such a scenario `(held)`. The implementer who adds the op, or extends the core test's list, lands the scenario with it, re-recorded. Each row becomes done in the change that lands its scenarios (re-recorded) together with the testkit support.

| Harvest id | Owner | Kind | Held scenarios |
|---|---|---|---|
| h0bc924c8e05f | U49, U87 | blocked | `notes/change-deck/memory-states` |
| h1145d1f031f2 | U49, U87 | blocked | `notes/change-deck/memory-states` |
| h2ed53ea93d68 | U87 | blocked | `notes/change-deck/memory-states` |
| h346f0091fba0 | U87 | blocked | `notes/change-deck/memory-states` |
| h55e8477eea21 | U49, U87 | blocked | `notes/change-deck/memory-states` |
| h5a6b3bd92841 | U49, U87 | blocked | `notes/change-deck/remaining-steps` |
| h66f7a2aeee4f | U49, U87 | blocked | `notes/change-deck/remaining-steps` |
| h87ed9c949919 | U87 | blocked | `notes/change-deck/fsrs-off` |
| hcc385fabbd1f | U87 | blocked | `notes/change-deck/target-state-kept` |
| h8fb237008b4c | U49, U87 | blocked | `notes/change-deck/memory-states`, `notes/change-deck/target-state-kept` |
| he1455a2bccbd | U49, U87 | blocked | `notes/change-deck/remaining-steps` |
| h29a2723c264d | U87 | blocked | `notes/change-deck/memory-states`, `notes/change-deck/remaining-steps`, `notes/change-deck/target-state-kept` |
| h28ba189d8c7e | U86 | needs-op | `notes/transform/replace-saves` covers the Find and Replace path only (card generation on, stored tag case kept, the stamp). Changed 2026-11-21 (implementation review): the test also asks for a transform with card generation off (no card added although a template now renders) and with registered tag spelling; only h83c15fb8bf40's `refresh_notes` op reaches those, so the row is linked to `replace-saves` together with that op's scenario when it lands |
| h3bc595fa00b0 | U86 | blocked | `browser/find-replace/field-names` |
| h6d2b664432d7 | U246, U86 | blocked | `browser/find-replace/mode-counts` |
| h22d21da07c9d | U86 | blocked | `tags/find-replace/listed-notes`, `tags/find-replace/pattern-modes` |
| h8838c90310d2 | U86 | blocked | `tags/find-replace/pattern-modes` |
| h0b434e5e6376 | U86 | blocked | `tags/find-replace/invalid-regex` |
| h102f0046f2fd | U86 | blocked | `tags/find-replace/all-notes` |
| hcd7b5a34f3ac | U86 | blocked | `tags/find-replace/listed-notes` |
| h1df457b7a669 | U246 | blocked | `sched/ops/set-due/missing-default-preset` |
| h6a74f5728f8c | U246 | blocked | `sched/ops/set-due/missing-preset` |
| h25a2a4a1f100 | U246 | blocked | `sched/counts/new-studied-today` |
| h8673919e1db4 | U246 | blocked | `sched/counts/nested-new-limits` |
| h2e7c980ee2f6 | U246 | blocked | `render/filters/kana-stored-edits` |
| h1126a583e576 | U246 | blocked | `render/type-answer/av-removal` |
| h21a1ea45b18d | U246 | blocked | `render/type-answer/av-removal` |
| h1b8a7b774a45 | U246, U41, U43, U44 | blocked | `sched/session/progress/new-note-again` |
| h56c0cea0f0c6 | U246, U41 | blocked | `sched/queue/limits/new-parent-child` |
| h69f8e4b9fca9 | U246, U41, U43, U44 | blocked | `sched/session/requeue/lapsed-review-due` |
| h6e88f0974790 | U246, U41, U43, U44 | blocked | `sched/session/progress/learn-ahead-finish` |
| h85d4fffafbab | U246, U41, U43, U44 | blocked | `sched/session/progress/answer-counts` |
| ha06ea8f4e666 | U246, U41, U43, U44 | blocked | `sched/session/head/counts-follow-answers` |
| ha57b6191cc59 | U246, U44 | blocked | `sched/steps/custom-steps-chain` |
| ha8b070f64ade | U246, U44 | blocked | `sched/answer/lapse-then-easy` |
| hade25a755a24 | U246, U41 | blocked | `sched/counts/missing-deck-card` |
| hc296a5b7e05e | U246, U41, U43 | blocked | `sched/queue/empty-collection` |
| hc4e4156f3624 | U246, U41, U43, U44 | blocked | `sched/session/head/sibling-order-unburied` |
| hdf5598f4cd0f | U246, U41, U43, U44 | blocked | `sched/session/deck-flow/parent-then-children` |
| he3a4a3e5014c | U246, U41, U43, U44 | blocked | `sched/session/deck-flow/child-review-limit` |
| head34b2562fb | U246, U41 | blocked | `sched/counts/shared-ancestors` |
| hf44d25b35e71 | U246, U44 | blocked | `sched/answer/review-ratings` |
| h05a411d3dbbc | U246, U43, U44 | blocked | `sched/session/undo/learning-counts` |
| h0cc7ca135634 | U246, U41, U43, U44 | blocked | `sched/session/deck-flow/new-within-review-limit` |
| h151bd9c1b80b | U246 | blocked | `sched/ops/reset/last-position`, `sched/ops/set-due/last-position` |
| h1d5bffd22663 | U246, U41 | blocked | `sched/queue/limits/new-limit-not-spent-by-reviews` |
| h23b15f93e355 | U155, U246, U41, U43 | blocked | `sched/counts/congrats-work-kinds` |
| h3f11a775e29c | U246, U41 | blocked | `sched/queue/limits/new-ignore-zero-review-limit` |
| h4833ddf59228 | U246, U41, U43, U44 | blocked | `sched/queue/bury/new-first-review-sibling` |
| h63e4063e58e3 | U155, U246, U41, U43 | blocked | `sched/queue/empty-collection` |
| h7bd9a5ed79b8 | U155, U246, U41, U43, U48 | blocked | `sched/counts/congrats-user-buried` |
| h9a5ca7ceecd6 | U246, U41, U43 | blocked | `sched/counts/congrats-work-kinds` |
| ha258d562e101 | U246, U43, U44 | blocked | `sched/session/undo/learning-counts`, `sched/session/undo/redo-after-deck-step` |
| had8092d884ac | U155, U246, U41, U43 | blocked | `sched/ops/set-due/congrats-review-today` |
| hb46e7a2d004c | U246, U44 | blocked | `sched/answer/previewed-states` |
| he7fa6992322b | U246, U41 | blocked | `sched/queue/limits/new-ignore-zero-review-limit` |
| hee2d6b23e65c | U246, U43, U44 | blocked | `sched/leech/undo-with-buried-sibling` |
| hf54cd97685d7 | U246, U41 | blocked | `sched/queue/limits/ancestor-new-limit` |
| hf55838aff7f6 | U246, U41 | blocked | `sched/queue/order/relative-overdueness` |
| h4c7fe3f35b39 | U41 | blocked | `sched/queue/order/relative-overdueness` |
