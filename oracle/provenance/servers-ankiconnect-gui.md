# AnkiConnect GUI actions, `sync` and `searchNote` navigation (U146)

Spec-writer notes for `specs/servers/ankiconnect-gui.md` G1-G12
(session s-20261110T085429Z-165d83d3, wave 2, package `ankiconnect-envelope`).
No code was copied. The reference build is the one identified in
`servers-ankiconnect-envelope.md` (upstream commit `532a3c8`, likely match
of the 2025-11-09 build). Anki files were read at tag 26.09.3
(`refs/anki-26.09.3`) only. No transcript exists yet (U136).

| Spec section | Source (version) | Read for |
|---|---|---|
| G1, G2 | plan R1782-R1788, U146, U150, U195, U210 | Meguri's single-window model, unsaved-sheet refusal, bring to front, review session, the 10 s interaction timeout |
| G3 | add-on `plugin/__init__.py` `guiBrowse` and `findCards` (532a3c8); README `guiBrowse`; Anki `qt/aqt/browser/table/table.py` (sort column change: the notice for an unsortable column, the first-direction override for a new descending-first column), `qt/aqt/browser/table/model.py` (column index only for active columns) | parameter shapes, the four reorder error texts and their order, the reply as the search's card ids, `[]` without a query, the column having to be displayed, search before validation in the reference |
| G4 | `plugin/__init__.py` `guiSelectCard` (532a3c8) | `false` only when no browser window exists, `true` otherwise |
| G5 | `plugin/__init__.py` `guiSelectedNotes` (532a3c8); Anki `qt/aqt/browser/browser.py`, `qt/aqt/browser/table/table.py`, `qt/aqt/browser/table/state.py` | `[]` without a browser; selection order as Qt reports it; distinct notes of selected cards |
| G6 | `plugin/__init__.py` `guiAddCards` (532a3c8); upstream history (commit `c53aa86`, 2022-04-07, the option `closeAfterAdding` ignored since); README `guiAddCards`; plan R1784, R1785, U27 | deck checked before note type, exact-name field matching, attachments stored before the window opens, the reply being the unsaved note's id (0), current deck and note type set at request time in the reference |
| G7 | `plugin/__init__.py` `guiEditNote`, `plugin/edit.py` (532a3c8); Anki `rslib/src/error/not_found.rs` | the reference's not-found text being Anki's own message; reply `null` |
| G8 | plan U135 (host-interaction channel), U144, U146; `specs/servers/host-security.md` H11.2, H20; `specs/sync/policy.md` P12.6 (outcome and reason names) | Meguri-designed (payload and answer JSON forms added in the consistency pass of 2026-11-10) |
| G9 | `plugin/__init__.py` `sync` (532a3c8); Meguri specs `sync/policy` P3, P12, P21 and `sync/status-strings` T6; plan R1754, U146 | the reference's `sync: auth not configured` text; the mapping of Sync Now's availability and results to replies |
| G10 | plan R1733, U140, U146 | Meguri-designed; the `/create` wire shape belongs to the Migaku package |
| G2 (timing, windows, review) | plan R1787, R1788, U146, U150, U175; Meguri specs `servers/host-security` H11.2, `sched/review-session` C1, C2 | Meguri-designed: timer from posting, the serial GUI kinds, the closed-window case |
| G3 (sort write, null, value text) | `plugin/__init__.py` `guiBrowse`, `findCards` (532a3c8: `None` defaults, the messages formatted with the runtime's string conversion); Anki `qt/aqt/browser/browser.py` (a failed search shows a modal warning) at 26.09.3; Meguri specs `browser/config-keys` K4.4, K5.2, K10.2, `engine/undo` M7 | null as absent; the reference's value text form; the sort as a `LocalApi` op is Meguri's |
| G4, G5 (selection details) | `plugin/__init__.py` `guiSelectCard` (clears the selection first), `guiSelectedNotes` (532a3c8); Anki `qt/aqt/browser/table/table.py`, `qt/aqt/browser/table/state.py` at 26.09.3 (cards-mode notes from a distinct query over the selected cards) | the G11 rows |
| G6 (member errors, media op) | Meguri specs `servers/ankiconnect-actions` A1, A7.1 and `servers/ankiconnect-media` X10.2 | Meguri-designed, aligned with `addNote` |
| G9 (params, waiting) | Yomitan `ext/js/comm/anki-connect.js` (public upstream, revision not recorded): `sync` called with `{version}`; Meguri spec `sync/policy` P12.3, P12.5, P15 | the G11 row; `sync: waiting` is Meguri's |
| G10 (digit string) | Meguri specs `servers/migaku-received-log` (1 to 19 digits), `search/grammar` G8 | Meguri-designed |
| G2 note (asbplayer) | asbplayer `common/anki/anki.ts` (public upstream, revision not recorded): the refresh option sends `guiBrowse` `nid:1`, then `nid:<id>` | non-normative note and open question |
| G11, G12 | derived from the rows above | |

## Interface strings reproduced from the reference

`reorderCards should be a dict: <value>`,
`Must provide a "columnId" and a "order" property"` (with its unbalanced final
quote), `invalid card order: <order>`, `invalid columnId: <columnId>`,
`sync: auth not configured`; the parameter names `query`, `reorderCards`,
`order`, `columnId`, `card`, `note`, `deckName`, `modelName`, `fields`,
`tags`, `options`, `closeAfterAdding`, `audio`, `video`, `picture`, and the
order values `ascending`, `descending`. The `note was not found`, `deck was
not found` and `model was not found` strings come from the Product Contract
(R1738). Every other string is Meguri's.

## Notes for U136

- The reference's unsortable-column path opens a modal notice that blocks the
  add-on until dismissed; do not record `browse-unsortable` against it.
- A query that fails to parse makes the reference's browser show a modal
  warning that blocks the add-on the same way: never record `browse-bad-query`
  or any other scenario with an invalid `guiBrowse` query.
- The G3 examples need `activeCols`
  `["noteFld","template","cardDue","deck","noteCrt"]` in the recorder's
  collection config before the first `guiBrowse`; `browse-reorder-default-cols`
  needs it absent. The reference only accepts a displayed column.
- Record `select-no-browse` and `selected-none` in a fresh Anki session, before
  any `guiBrowse` or `guiEditNote` scenario: those leave a browser window open,
  after which the reference replies `true` and the selection.
- `guiAddCards` and `guiEditNote` open Qt windows; under the offscreen
  platform they should still reply, but record them last in a profile or
  restart Anki between them so a left-open window cannot change later
  replies.
- `sync-not-connected` needs a profile without a sync login.

## Probes against the running add-on (2026-11-10)

Session and setup as in `servers-ankiconnect-envelope.md` "Probes against the
running add-on": AnkiConnect 25.11.9.0 in Anki 26.9.3 offscreen, frozen clock
1894701600. Collection: deck German with two Basic notes, one Basic note in
Default, default Browse columns. No modal dialog was triggered (no invalid
query, no unsortable column).

| Spec text | Probe and result |
|---|---|
| G2 value text | `reorderCards` `['noteCrt']` and `x`; `order` `5`, `True`; `columnId` `['noteCrt']`: texts in the runtime's form |
| G3 | `{}` and `{"query": null, "reorderCards": null}` reply `[]`; `order up` with `columnId nope` gives the order error; `noteCrt` and `cardLapses` with default columns, and `nope`, give `invalid columnId: <id>`; valid sorts on `deck`, `cardDue`, `noteFld` reply the ids |
| G11 bad reorder | after a bad `reorderCards` on `deck:Default`, selecting the Default card then `guiSelectedNotes` gave that note, so the search ran |
| G11 non-string query | `query: 5` fails with a Qt runtime text |
| G4 | before any browser: `false`; afterwards `true` for an outside card, an unknown id and `"1"`, with no row selected after |
| G5 | before any browser `[]`; after selecting a card, its note |
| G6 | unknown deck, unknown model, both unknown (deck reported); missing `modelName` and a non-string field value give runtime texts; valid note, `{}` and `note: null` reply `0` in 0.02-0.2 s; a second `guiAddCards` with the window open replied `0` at once |
| G7, G11 | unknown id: Anki's database-inconsistency message naming the id; an existing note: runtime error from building the edit window under Anki 26.9.3, so `edit-valid` became a Meguri row |
| G9 | no sync login: `sync: auth not configured`; `params {"version": 2}`: unexpected-keyword text |
| Not probed | sort keys after a `reorderCards` (the collection is locked while Anki runs, so its config could not be read); the first-direction rule; filtered decks; window activation |

## Review fixes (2026-11-10, second pass)

Same session; probe container `acenv-r02-81080` (removed afterwards).

| Spec text | Source and result |
|---|---|
| G11 open Add window (R0.0) | source-derived, not observable offscreen: Anki 26.09.3 `qt/aqt/addcards.py` (the close-with-callback and if-can-close methods, and the discard-or-keep question shown through the callback-based ask-user dialog) and `ts/routes/editor/NoteEditor.svelte` (the should-prompt-before-closing check, true for any non-empty field). The add-on closes the current Add window with a callback and returns the note id at once. Only the immediate `0` reply was observed (G6 row above) |
| G2, G11, G12 `browse-order-null` (R0.5, R1.3) | `order` values `-0`, `null`, `5.0`, `1e2`, `true` gave `invalid card order: 0`, `None`, `5.0`, `100.0`, `True` |

## Transcript recordings (2026-11-17)

Session s-20261117T160047Z-35f65b90 (spec writer, servers transcripts).
Pins: Anki tag 26.09.3 (`refs/anki-26.09.3`) and the AnkiConnect add-on build
pinned in `oracle/ankiconnect/ADDON_PIN` (tag 25.11.9.0, commit `532a3c8`,
build 2025-11-09), installed as `addons21/2055492159` in the recorder image
`meguri-oracle-ankiconnect:recorder`. No code was copied into a spec or a
scenario.

| Subject | Sources | Read for |
|---|---|---|
| G12 scenarios under `conformance/recordings/servers/ankiconnect/gui/` | Transcript oracle recordings only: the AnkiConnect add-on 25.11.9.0 (build 2025-11-09) in Anki 26.9.3, image `meguri-oracle-ankiconnect:recorder`. No Anki or AnkiConnect source was read. | Checking the 20 recorded replies against G3 to G6 and G9; every one matched |
