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
