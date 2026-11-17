# AnkiConnect collection actions (U142)

Spec-writer notes for `specs/servers/ankiconnect-actions.md` A1-A21
(session s-20261110T085429Z-165d83d3, wave `servers`, package
`ankiconnect-actions`). No code was copied. The first pass ran no oracle
probe, so the replies in A20 came from reading the sources below; the probe
section at the end records what was then observed on the pinned build. The
goldens U136 records stay the authority. The add-on
build of 2025-11-09 was not available; its closest public source (below) was
read instead, and A21 Q6 leaves the final word to U136's `ADDON_PIN`
recording.

Sources:

- AnkiConnect add-on (GPL-3.0-or-later, third party, read only): public
  repository `https://git.sr.ht/~foosoft/anki-connect`, cloned into the
  session scratchpad, checked out at commit
  `532a3c8` (2025-11-06, "Added guiPlayAudio"), the last public commit before
  the installed build's date of 2025-11-09 (the next one is 2025-12-05). The
  installed build itself was not available on this machine; A21 Q6 asks U136
  to confirm the match with `ADDON_PIN`. Upstream `master` (2026-10) has moved
  to a different API generation and was not used.
- Anki at tag 26.09.3 (`refs/anki-26.09.3`), for what the add-on's calls do.
- `local-data/research/09-ankiconnect.md` §1, §2, §4, §5 (action list, client
  inventory); no private collection content was used.
- The plan: Product Contract R1734-R1801, units U141, U142, U143, U136;
  `docs/plans/research/coverage/servers.md`.

| Spec section | Sources read | Read for |
|---|---|---|
| A1, A2 | AnkiConnect `plugin/__init__.py` (request handler, note and card lookups), `plugin/util.py`; Anki `pylib/anki/errors.py`, `pylib/anki/notes.py` (`remove_tag`), `pylib/anki/tags.py` (`in_list`) | missing-key errors surfacing as Python `KeyError` text, `str(list)` of errors, the not-found texts with capital `Note`/`Card` and their origin in the backend error's message; the lower-case form (Python `str.lower()` on both sides) behind A1's "match ignoring case" |
| A3.1 | AnkiConnect `plugin/__init__.py` (every `@util.api` signature) | which parameters have defaults (`cardsToo`, `add`, `complete`, `suspend`, `css`, `isCloze`, `query`, `notes`); whether a parameter is required, types and null handling are Meguri's (`servers/ankiconnect-envelope#E12.2`) |
| A3 | AnkiConnect `plugin/__init__.py` (every `@util.api` method) | parameter names and defaults (`suspend`, `add`, `cardsToo`, `complete`, `css`, `isCloze`), return values (`null` for most writes, `true` for `setDueDate`), which actions write |
| A4 | AnkiConnect `getProfiles`, `getActiveProfile`, `loadProfile`, `reloadCollection` | result shapes |
| A5.1, A5.2 | AnkiConnect `deckNames`, `deckNamesAndIds`; Anki `pylib/anki/decks.py` (`all_names_and_ids`, `id`), `rslib/src/decks/service.rs`, `rslib/src/decks/name.rs`, `rslib/src/storage/deck/mod.rs` | all decks including an empty Default and filtered decks, ordered by stored name |
| A5.3, A5.7 | AnkiConnect `getDecks`, `deckNameFromId`; Anki `pylib/anki/decks.py` (`get` with its default-deck fallback) | unknown card and deck ids answered with the default deck's name |
| A5.4, A5.5 | AnkiConnect `createDeck`, `changeDeck`; Anki `pylib/anki/decks.py` (`id` creating missing decks), `pylib/anki/scheduler/legacy.py` (return from filtered decks) | create-or-return-id, creation of the target deck, raw `did` rewrite of every listed card |
| A5.6 | AnkiConnect `deleteDecks` | the `cardsToo` error text, exact-name filter re-evaluated per deck, one removal per deck |
| A5.8 | AnkiConnect `getDeckStats` and its tree helpers; Anki `pylib/anki/scheduler/base.py` (`deck_due_tree`), `proto/anki/decks.proto` (`DeckTreeNode`), `rslib/src/decks/tree.rs`, `rslib/src/storage/deck/due_counts.sql` | members, node name as the last component, limited counts, `total_in_deck` as all cards of the deck without children, preorder, decimal-string keys, deck creation for unknown names |
| A6 | AnkiConnect model actions; Anki `pylib/anki/models.py` (`by_name`, `get`, `new`, `new_field`, `new_template`, `add`, `ensure_name_unique`, `_mutate_after_write`, `all_names_and_ids`), `rslib/src/notetype/service.rs` (`get_notetype_names`, no sort), `rslib/src/storage/notetype/mod.rs`, `rslib/src/storage/notetype/get_notetype_names.sql` (no `ORDER BY`), `rslib/src/storage/upgrades/schema15_upgrade.sql` (`name ... COLLATE unicase`, unique index `idx_notetypes_name`) | name lookup under `unicase`; names in `unicase` name order, because SQLite answers the unordered two-column query from the covering name index (a reviewer reproduced the plan with `EXPLAIN QUERY PLAN` on that schema; the golden confirms); legacy dict returned after the add, the clash suffix, template default names counting every position, the three `createModel` errors; `rslib/src/notetype/mod.rs` (`check_templates`) for templates Anki refuses at save |
| A6.1 | AnkiConnect `modelFieldsOnTemplates` | which simple tags are listed (body without `#`, `/`, `}`, lazy match, so `{` may be inside), every brace removed before the last-colon cut, FrontSide and question-side exclusions; the table rows were worked out from that reading |
| A7 | AnkiConnect note creation and its duplicate check, `addNote`, `addNotes`, `canAdd*`; Anki `pylib/anki/notes.py` (`fields_check`, `__contains__`, no `__len__`), `pylib/anki/collection.py` (`addNote`, `add_note`), `rslib/src/notes/mod.rs` and `rslib/src/notetype/cardgen.rs` (a note always gets a card, so the add-on's "empty question on all cards" error never fires), `rslib/src/template.rs` | step order of A7.2 (model member, model lookup, deck member, deck lookup, tags, fields, attachments, options, check), field matching by lower-case forms (the add-on lower-cases both names; it differs from `unicase` for `Straße`/`STRASSE`), default check through the field-check states including the "unknown reason" states, scoped check on raw first field with checksum only and card decks (an unknown scope deck making nothing a duplicate), option type errors, no type check on `duplicateScopeOptions` or its `deckName`, attachments before the check, add-then-delete rollback of `addNotes`, `canAdd*` hiding every error. The A7.3 outcome table was derived from these rules, not recorded |
| A8 | AnkiConnect `updateNoteFields`, `updateNote`, `updateNoteTags`, `getNoteTags`, `notesModTime`, `deleteNotes`; Anki `pylib/anki/tags.py` (`bulk_add`, `bulk_remove`, `split`) | case-sensitive field names, the `updateNote` and tags error texts, tag replacement by remove-all then add-each |
| A9 | AnkiConnect `findNotes`, `notesInfo`; Anki `pylib/anki/collection.py` (`find_notes` without order) | `[]` for a missing query, `{}` in place, cards by template ordinal, profile member; a note without cards raising an uncaught missing-key error (`ankiconnect-notesinfo-no-cards`) |
| A10 | AnkiConnect tag actions; Anki `pylib/anki/tags.py` (`all`, legacy `registerNotes`, `bulkAdd`, `in_list`), `pylib/anki/notes.py` (`has_tag`, `remove_tag`, `add_tag`), `rslib/src/tags/register.rs` (`canonify_tags` splitting each item and dropping empty pieces), `rslib/src/storage/tag/get.sql`, `rslib/src/storage/upgrades/schema17_upgrade.sql` | registry order (primary key under `unicase`), clear-unused mapping, whole-tag replacement with Python `str.lower()` on both sides, a replacement item split at separators on save |
| A11 | AnkiConnect `findCards`, `cardsInfo`, `cardsModTime`, `cardsToNotes`; Anki `pylib/anki/cards.py` (`question`, `answer`), `pylib/anki/template.py` (render output, style prefix, error output with empty CSS, the `card_did_render` hook run outside the browser), `rslib/src/template.rs` (blank notices in both sides), `pylib/anki/latex.py` (the only render hook), `pylib/anki/collection.py` (installs the LaTeX hook at import) | members and their columns, `fieldOrder` equal to `ord`, the composition of question and answer, next-state labels, LaTeX rewritten to image tags in AnkiConnect's replies (`ankiconnect-cardsinfo-latex`) |
| A12 | AnkiConnect `suspend`, `unsuspend`, `suspended`, `areSuspended`, `areDue`, `getIntervals`; Anki `pylib/anki/scheduler/base.py`, `rslib/src/scheduler/bury_and_suspend.rs` | the result of `suspend`: the add-on removes already-matching cards from the list while looping over it, which skips the element after each removal, so a list of two or more never empties (A12.1) and, in `unsuspend`, a buried card after a removed one survives the filter and is restored with the others (`ankiconnect-unsuspend-buried`); `unsuspend` returning nothing; the card states behind A12.4 (a new-card search, the latest review-log row, the 1200-second threshold, a due search); `IndexError` text for cards without review-log rows. The A12.4 example table was derived from these rules |
| A13 | AnkiConnect `forgetCards`, `setDueDate`; Anki `pylib/anki/scheduler/base.py` (`set_due_date`) | Reset with restore position on, counts kept, no context; Set Due Date without config key; `true` result |
| A14 | AnkiConnect `getNumCardsReviewedToday`, `getNumCardsReviewedByDay` | the strict window start, every row counted, local dates shifted by the cutoff's local hour, newest first |
| A15 | Meguri design (R1789, R1790, `engine/op-model`), no AnkiConnect source | one op per write, snapshot reads |
| A16 | AnkiConnect search actions; Anki `pylib/anki/collection.py` (`find_cards`, `find_notes`) | no ordering requested, raw search text |
| A17 | Meguri design (proposed wording), no Anki or AnkiConnect source | |
| A18 | the rows above; Anki `pylib/anki/latex.py`, `pylib/anki/collection.py` for `ankiconnect-cardsinfo-latex`; AnkiConnect `setDueDate`, `createDeck` (Anki error texts passed through) for `ankiconnect-error-text` | where Meguri differs |
| A19 | `local-data/research/09-ankiconnect.md` §4; `specs/servers/ankiconnect-clients.md` C2, C3 | the client inventory, agent sessions and scripts included |
| A20 | Meguri design from the sections above and the plan's U142 test scenarios; the scenario note types `Pair`, `Gap`, `Rev` are Meguri's own (R1959); tokens per `specs/servers/transcripts.md` T10.1 | expected replies derived from reading, not recorded |
| A21 | the plan's Product Contract (R1738, R1758, R1762, R1767, R1774) against the add-on's behavior | the open decisions |

Probe of the pinned add-on (2026-11-10, same session, wave 5 item
`ac-actions`): AnkiConnect 25.11.9.0 (`ADDON_PIN`, commit `532a3c8`) in
Anki 26.9.3, image `meguri-oracle-ankiconnect:spike`, run as
`oracle/ankiconnect/README.md` says (internal network, tmpfs `/scratch`,
`ORACLE_CLOCK_START=1894701600`, `TZ=Europe/Paris`, no published port), probed
with `docker exec` and Python `urllib` to `127.0.0.1:8765`. Containers and
network were named `ac-actions-*` and removed afterwards. Setup went through
the add-on's own actions (`createModel`, `createDeck`, `addNote`,
`insertReviews`, `answerCards`); for states no action makes (a filtered deck,
user and scheduler buried cards, learning cards with chosen review-log rows, a
note without cards, a card without its note, templates a checked save
refuses) a throwaway probe add-on in the container's tmpfs ran Anki's Python
API on the main thread. The collection was read from a copy of its file. No
private data was used; every value was synthetic. Probe scripts and raw
outputs stayed in the session scratchpad and are not committed.

| Spec section | Observed |
|---|---|
| A4 | profile list, active profile, exact-name `loadProfile`, `reloadCollection` null |
| A5 | deck name order, `deckNamesAndIds`, `getDecks` (unknown id under Default, filtered decks by `did`), `createDeck` ids under the frozen clock (middle level lowest), `changeDeck` creating the target, stamping cards already there, accepting a filtered target, `deleteDecks` texts and exact names, `deckNameFromId` fallback, `getDeckStats` members, preorder, zero-count creation of unknown names |
| A6, A6.1, A6.2 | note type reads, not-found texts, every A6.1 row (two through an unchecked write), `createModel` check order and texts, `Card <n>` names, the `-` + five characters clash name, stock CSS for missing or null `css`, `curModel` set |
| A7 | step order, member texts (runtime texts), string `tags` accepted, non-object `options` ignored, `duplicateScopeOptions` null error, default and scoped checks (A7.3 table and `dupe-scope` rows), Python list form of `addNotes` errors, rollback leaving graves and moving `nextPos`, add defaults written |
| A8 | `Note was not found: <id>` from four actions, case-sensitive `updateNoteFields`, `updateNote` and `updateNoteTags` texts |
| A9 | `findNotes` without query, `notesInfo` members, `query` winning over `notes`, the missing-key text for a note without cards |
| A10, A10.1 | tag add, remove, registry, clear, every replace row |
| A11 | `cardsInfo` member order, render outcomes, LaTeX tag and message, isolation marks in `nextReviews`, `cardsModTime`, `cardsToNotes` |
| A12 | the `suspend` result table, unknown-id behavior, `unsuspend` null and the position-dependent unburying, `areDue` and `getIntervals` rows, learn-ahead |
| A13 | invalid days text, missing ids skipped, repeated ids database error, `type` 4 rows |
| A14 | counts including types 4 and 5, rollover dates |
| A16, A18 | scan order, ASCII-only case folding for `über`, search error text; every A18 row except the step-remap and FSRS points of `ankiconnect-changedeck-op` and the too-deep error |

Not probed: `notes/preservation`, `cards/reschedule` against the UI ops,
`search/current` with cards, the too-deep deck error.

Similarity: run `tools/mg similarity` from the main checkout against the
AnkiConnect corpus U136 adds before merging; this worktree has no corpus.

Review fixes (same session): the ordered branch tables of A12.4/A12.5, the
scoped duplicate steps of A7.3 and the pattern wording of A6.1 were recast as
card-state rules, rules plus outcome tables, and template-tag rules; the
add-on's internal mechanisms they summarize are recorded here only.

Second review fixes (same session, 2026-11-10): re-probed on the pinned
image in a throwaway container and internal network (`acrev-*-9c1e`, both
removed), setup through the add-on's actions. Observed: `addNote` with
`tags: "pq rs"` stored the tags `p q r s` (the add-on assigns the string to
the note's tag list unconverted, so it is read one character at a time);
`options` 5, true, `{duplicateScopeOptions: 7}` failed with
`argument of type '<int|bool>' is not iterable`; `options` `"allowDuplicate"`,
`"duplicateScope"` and `{duplicateScopeOptions: "deckName"}` failed with
`string indices must be integers, not 'str'`; `["allowDuplicate"]` and
`{duplicateScopeOptions: ["deckName"]}` failed with
`list indices must be integers or slices, not str`; `"xyz"`, `[]`,
`{duplicateScopeOptions: "xyz"}` and `{duplicateScopeOptions: []}` were
ignored and the note added (the add-on tests membership with Python `in`
before indexing); `tags: ["a", "OLD", "old"]` stored `a OLD`, then
`replaceTags old -> z` gave `a z`; `createDeck A::B::C` gave `A::B`, `A`,
`A::B::C` ids in that order, as `specs/decks/decks.md` D3 states. Written
into A7.1, A7.4, A10.1, A18 `ankiconnect-note-member-types`, A20.1
`create-nested`, A20.3 `add-errors` (four Meguri rows), A21 intro, Q9 and the
new Q10 (`list index out of range` is the runtime's `IndexError` text, kept as
an interim).

Transcript alignment (2026-11-10): no new Anki source. The header and A20
follow `specs/servers/transcripts.md` T2 (scenario paths under
`conformance/recordings/servers/ankiconnect/`), T18 and
`specs/servers/ankiconnect-clients.md` C5 (`{db_dump: {tables: [notes, cards]}}`
and `media_dir`). The proposed A18 row `ankiconnect-nextreviews-isolation-marks`
was dropped and A11.4 reworded: T10.3 removes U+2068 and U+2069 from
`nextReviews` items, so the observed marks (A11 row above) need no deviation.

## Transcript recordings (2026-11-17)

Session s-20261117T160047Z-35f65b90 (spec writer, servers transcripts).
Pins: Anki tag 26.09.3 (`refs/anki-26.09.3`) and the AnkiConnect add-on build
pinned in `oracle/ankiconnect/ADDON_PIN` (tag 25.11.9.0, commit `532a3c8`,
build 2025-11-09), installed as `addons21/2055492159` in the recorder image
`meguri-oracle-ankiconnect:recorder`. No code was copied into a spec or a
scenario.

| Subject | Sources | Read for |
|---|---|---|
| A5 deck actions (`createDeck`, `changeDeck`, `deleteDecks`, `getDecks`, `deckNameFromId`, `getDeckStats`) and the missing filtered-deck creation | Pinned AnkiConnect add-on 25.11.9.0 in `meguri-oracle-ankiconnect:recorder` at `/opt/ankiconnect/addons21/2055492159/__init__.py` (deck actions); `oracle/driver` `setup_section` registrations; `oracle/ankiconnect/recorder/transcript.py` `STEP_KINDS` | Confirming that the recorded behavior matched A5 (no correction needed) and that no setup section or action can make a filtered deck in a transcript. Nothing was copied into the spec or scenarios. |
| A6.2 `createModel`: check order and template naming | AnkiConnect add-on 25.11.9.0 (`ADDON_PIN`, installed as `addons21/2055492159` in `meguri-oracle-ankiconnect:recorder`), `plugin/__init__.py`, `createModel` | To confirm why the golden shows `inOrderFields`, then `cardTemplates`, then the exact-name check, then the `Front` and `Back` member errors per template, and the `Card <n>` default name. No spec text was taken from it, and the recorded behavior matches A6.2. |
| A20.4 `tags/` transcripts (A10, A10.1, A8.3, A8.4) | Oracle recordings only, made with `oracle/ankiconnect/record.sh` and image `meguri-oracle-ankiconnect:recorder` (AnkiConnect 25.11.9.0 in Anki 26.9.3). One extra probe was run by hand from the scratchpad and not committed. No Anki or AnkiConnect source was read. | Recording goldens and checking A10's statements: tag splitting, child removal, the registry after `clearUnusedTags`, whole-tag case-insensitive replace, the `updateNoteTags` type error, `getTags` unicase order and the list-typed `tags` error |
| A20.5 `cards/` scenarios (A11, A12, A13 card actions; A18 `ankiconnect-repeated-ids`) | AnkiConnect add-on 25.11.9.0 inside `meguri-oracle-ankiconnect:recorder` at `/opt/ankiconnect/addons21/2055492159/__init__.py` (`cardsInfo`, `cardsModTime`, `cardsToNotes`, `suspend`, `unsuspend`, `suspended`, `areSuspended`, `areDue`, `getIntervals`, `forgetCards`, `setDueDate`, `updateNoteFields`, `setSpecificValueOfCard`, `insertReviews`, `answerCards`). Anki 26.09.3: `rslib/src/scheduler/reviews.rs` (set_due_date FSRS branch); `rslib/src/notetype/mod.rs` (template checks at save, special field names); `rslib/src/template.rs` (render-time conditional evaluation); `rslib/src/notetype/render.rs` (special fields put into the render map) | Why `setDueDate` leaves `ivl` 0 under FSRS. Which actions can prepare card state, and which of them Meguri refuses. Finding a template that a checked save accepts and that fails at render. Why a repeated id makes `forgetCards` write two rows and makes `setDueDate` fail on a UNIQUE constraint. How `cardsToNotes` selects distinct note ids. |
| A20.6 and A20.7 recordings (2026-11-17) | Recorder image `meguri-oracle-ankiconnect:recorder` (add-on 25.11.9.0, archive sha256 `cf75820e…`, Anki 26.9.3), run through `oracle/ankiconnect/record.sh`. Each of the 7 oracle scenarios was recorded twice in fresh containers, with byte-identical goldens. No Anki or AnkiConnect source was read: every golden matched the spec, so nothing needed explaining. | Confirming A16 (folding, scan order, error text, `deck:current`), A9.2 (`notesInfo` by query) and A14 (rollover dating, the strict today window with a row at exactly S excluded, type 4 rows counted) against the pinned add-on. |
