# AnkiConnect GUI actions, `sync` and Migaku `searchNote` navigation

Spec: servers/ankiconnect-gui
Requirements: R1754 (`sync`), R1782, R1783, R1784, R1785, R1786, R1787, R1788, R1733 (`searchNote` navigation); supports R1549, R1797
Anki: 26.09.3
Scenarios: none yet in `conformance/`. The transcript format is `servers/transcripts`; until its runner exists (U136), G12 lists the planned scenarios as request and reply tables, to become `conformance/scenarios/servers/ankiconnect/gui/` (reply shapes only).

The five GUI actions Meguri implements on its single-window Mac app (`guiBrowse`, `guiSelectCard`, `guiSelectedNotes`, `guiAddCards`, `guiEditNote`), the `sync` action, and the Browse navigation that the Migaku server's legacy `/create` `searchNote` command asks for. For each: parameters, the checks the core makes, the host interaction it raises, and the reply.

Targets: AnkiConnect API version 6, as served by the AnkiConnect add-on build of 2025-11-09 ("the reference"). The reference opens separate Qt windows; Meguri navigates its one main window instead, so window behavior is Meguri's (R1782–R1788) and only reply shapes are compared with recordings. G11 lists the differences. The reference replies stated in this file were checked against that build running in the transcript oracle (Anki 26.9.3, offscreen), except the G11 rows on modal notices, on the first sort direction, on filtered decks and on the question an open Add window asks before it is replaced, which would block the oracle, need a setup it does not have yet or cannot be seen offscreen; where a check changed the text, the section says "observed".

Everything before the action runs (origin, key, registry lookup, read-only mode, parameter checks, the 30 s collection wait, reply wrapping and error envelopes) is `servers/ankiconnect-envelope` (E3). Registry flags of these actions are its E12.3. Searches are Browse's (`search/query-model`, `browser/result-sets`); column keys and sorting are `browser/config-keys` K3 to K6. The host-interaction channel is `servers/host-security` H11.2 (U135).

## G1. Terms

- **GUI action**: one of the five actions above. **Navigating action**: `guiBrowse`, `guiAddCards`, `guiEditNote`; they change what the main window shows.
- **Browse showing**: the main window is open, minimized or not, and its current destination is Browse.
- **Unsaved sheet**: a sheet or panel presented over the main window that holds changes the user has not saved (deck options, note type editing and the like), as the navigation contract of U150 reports it. The Add panel holding an unsaved draft (U195) is an unsaved sheet for every GUI action except `guiAddCards`, and for `BrowseNote` (G10); for `guiAddCards`, G6 handles the draft.
- **The app's answer**: the outcome the Swift side gives to a host interaction (G8).
- **Bring to front**: the setting "Allow API to bring Meguri to front", on by default (R1787).

## G2. Rules shared by the GUI actions

1. Parameters are checked first (envelope E12.2 and each action's own checks below). A request that fails a check changes nothing in the window.
2. Collection GUI actions (`guiBrowse`, `guiAddCards`, `guiEditNote`) then wait for the collection (envelope E14) and do their collection work in the core: searching, resolving names and ids, storing attachment media, writing the Browse sort. An error there is replied without navigating. The core releases every reader and handle and ends its op before it posts the interaction (`servers/host-security` H11.2).
3. The core then posts one GUI interaction (G8) and waits for the app's answer. The 10 s timeout runs from the moment the core posts it, whatever the app is doing. The app performs the GUI kinds (`GuiBrowse`, `GuiSelectCard`, `GuiSelectedNotes`, `GuiAddCards`, `GuiEditNote`, `BrowseNote`) one at a time in the order they were posted; `PermissionPrompt` and `SyncNow` are not part of that order. When the timeout passes first, the reply is `cannot navigate: Meguri did not respond`, the host posts interaction cancelled (H11.2) and a later answer is not accepted. The app does not start a cancelled interaction; one already started finishes, and its window change stays. A GUI request therefore waits at most 30 s for the collection plus its collection work plus 10 s.
4. While an unsaved sheet is open, every GUI action fails with `cannot navigate: close the open sheet first`, and the sheet, its edits and the window are unchanged (R1787). The app reports this as the `refused` answer.
5. A navigating action that is not refused restores the main window when it is minimized or closed and activates Meguri when bring to front is on. When it is off, Meguri is not activated and a minimized window stays minimized; a closed main window is opened (not minimized) without activating Meguri, so the navigation has a window to happen in (R1787). `guiSelectCard` and `guiSelectedNotes` never change the window state or activation.
6. When a review session is in progress, a navigating action leaves it exactly as the user leaving the review would (the owners are U150 and U175; the session ends as `sched/review-session` C1 says), and returning to the deck starts a session normally (`sched/review-session` C2) (R1788). The answer of the current card is never recorded by the navigation. What the screen shows on the way out is the U150/U175 exit's, not this file's (see Open questions).
7. The reply follows the envelope's version rules. GUI actions add nothing to the undo history and never clear it. The only collection writes they make are `LocalApi` ops (envelope E14): `guiAddCards`' attachment media (G6) and `guiBrowse`'s sort (G3), which truncate the undo history only where they intersect it (`engine/undo#M7`, R1796).

Note (not normative): clients call navigating actions as part of their own work. asbplayer, with its "refresh browser after update" option, follows every note update with two `guiBrowse` calls (`nid:1`, then `nid:<id>`); against the reference these refresh a separate browser window, while in Meguri each switches the main window to Browse, brings it to front under rule 5 and leaves a review session under rule 6. See Open questions.

Error strings of this file:

| Message | When | Source |
|---|---|---|
| `cannot navigate: close the open sheet first` | rule 4 | R1787 |
| `cannot navigate: Meguri did not respond` | rule 3 | Meguri |
| `note was not found: <id>` | G7 | R1738, R1786 |
| `deck was not found: <name>` | G6 | R1738 |
| `model was not found: <name>` | G6 | R1738 |
| `reorderCards should be a dict: <value>` | G3 | reference prefix; `<value>` as below |
| `Must provide a "columnId" and a "order" property"` | G3 | reference text, including its unbalanced final quote |
| `invalid card order: <order>` | G3 | reference prefix; `<order>` as below |
| `invalid columnId: <columnId>` | G3 | reference prefix; `<columnId>` as below |
| `Meguri: the API is in read-only mode` | G3 | envelope E13 |
| G9's `sync: …` strings | G9 | Meguri, except `sync: auth not configured` |

`<value>`, `<order>` and `<columnId>`: a string is written as it is, with no quotes. Any other value is written as compact JSON: no white space, object members in the order received, strings with only the escapes JSON requires (`"`, `\` and the controls below U+0020, as `\"`, `\\`, `\n`, `\r`, `\t`, `\b`, `\f`, else `\u` and 4 lower-case hex digits), other characters as raw UTF-8, numbers as received. The reference writes non-string values in its language runtime's text form of the parsed value instead (observed: `['noteCrt']`, `True`, `None` for `null`, `5`, `0` for `-0`, `5.0` for `5.0`, `100.0` for `1e2`). So the texts are equal only for strings and for integers written in plain digits, with an optional `-`, other than `-0` (G11).

## G3. `guiBrowse(query?, reorderCards?)` (R1782)

| Parameter | Type | Default |
|---|---|---|
| `query` | string; `null` counts as absent | absent: Browse keeps its current search |
| `reorderCards` | any (envelope E12.2; the checks below), an object `{order, columnId}`; `null` counts as absent | absent: the sort is left as it is |

Checks, in this order (the first failure is the reply):

| # | Check | Error |
|---|---|---|
| 1 | `query`, when present, is a string | `invalid parameter: query` |
| 2 | `reorderCards`, when present, is an object | `reorderCards should be a dict: <value>` |
| 3 | it has both members `columnId` and `order` | `Must provide a "columnId" and a "order" property"` |
| 4 | `order` is `ascending` or `descending` | `invalid card order: <order>` |
| 5 | when `reorderCards` is present, read-only mode is off (envelope E12.3, E13) | `Meguri: the API is in read-only mode` |
| 6 | (after the collection wait) `columnId` is a column key displayed in Browse's current mode: one of the displayed columns of `browser/config-keys#K4.1` for the mode of `#K6.1`, as stored in the collection | `invalid columnId: <columnId>` |
| 7 | `query`, when present, parses and runs | the Browse error message (`search/errors#E7`) |

- The reply is the card ids that `findCards` returns for `query` (`servers/ankiconnect-actions`: the same search engine, ascending ids, R1776, R1777), whatever mode Browse is in. Without `query` the reply is `[]`, as in the reference (observed for `{}` and for `{"query": null, "reorderCards": null}`). The reference checks `order` before `columnId` (observed: `order` `up` with `columnId` `nope` gives `invalid card order: up`), as rows 4 and 6 do.
- **The sort.** With `reorderCards` and a column sortable in the mode, the core writes the sort as `browser/config-keys#K5.2` "Set sort (`columnId`, `order`)" does (the mode's column and backwards keys, each written only when it changes), as one `LocalApi` op (envelope E14), not as the UI's `UserNonUndoable` write of `#K10.2`: no undo entry, and the undo and redo history is not cleared (G2 rule 7). It commits before the interaction is posted. When the column is not sortable in the mode (`question`, `answer`; `stability`, `difficulty`, `retrievability` in notes mode), nothing is written, the sort is left unchanged and the action still succeeds, with no alert. The reference also keeps its sort and replies the ids, but only after the user dismisses a modal notice (G11).
- The interaction asks the app to show Browse with `query` as its search text and run it as if the user had entered it (`browser/result-sets#H3`); Browse shows the sort the collection now holds. When the interaction is refused or times out, a sort already written stays.
- The mode (cards or notes) is not changed.
- The answer `done` gives the reply; `refused` gives G2 rule 4's error.

Examples (collection: notes `n1`, `n2` in deck German with cards `c1` < `c2`; `c3` in Default; Browse in cards mode, `activeCols` `["noteFld","template","cardDue","deck","noteCrt"]`, so Created is displayed and Lapses is not; sort as stored by default, `noteCrt` ascending):

| Params | Reply | Window |
|---|---|---|
| `{"query": "deck:German"}` | `[c1, c2]` | Browse, search `deck:German` |
| `{"query": "deck:German", "reorderCards": {"order": "descending", "columnId": "noteCrt"}}` | `[c1, c2]` | sorted by Created, descending |
| `{}` | `[]` | Browse, previous search kept |
| `{"query": "deck:German", "reorderCards": {"order": "up", "columnId": "noteCrt"}}` | error `invalid card order: up` | unchanged |
| `{"query": "x", "reorderCards": {"order": "ascending"}}` | error `Must provide a "columnId" and a "order" property"` | unchanged |
| `{"query": "x", "reorderCards": ["noteCrt"]}` | error `reorderCards should be a dict: ["noteCrt"]` (Meguri's JSON form) | unchanged |
| `{"query": "x", "reorderCards": {"order": "ascending", "columnId": "cardLapses"}}` (Lapses not displayed) | error `invalid columnId: cardLapses` | unchanged |
| `{"query": "x", "reorderCards": {"order": "ascending", "columnId": "question"}}` (with `question` added to `activeCols`) | the ids for `x` | Browse on `x`, sort unchanged |
| `{"query": null, "reorderCards": null}` | `[]` | Browse, previous search and sort kept |
| read-only on; `{"query": "deck:German", "reorderCards": {"order": "descending", "columnId": "noteCrt"}}` | error `Meguri: the API is in read-only mode` | unchanged |
| `{"query": "("}` | the Browse error for `(` | unchanged |

## G4. `guiSelectCard(card)` (R1783)

| Parameter | Type |
|---|---|
| `card` | integer, required |

- Not a collection action: no wait.
- The interaction asks the app to select the row of card `card` in Browse's current results: in cards mode the card's row, in notes mode its note's row. The selection becomes that one row; the editor follows the selection as when the user clicks the row.
- Reply `true` when Browse is showing and the row is in the results and is now selected; `false` when Browse is not showing or the card is not in the results (also for an id no card has). With `false` the selection is left as it was. The window and the search do not change. Difference: the reference clears its browser's selection and replies `true` whenever its browser window exists, whether or not the card is in its results (G11). Observed: `false` before any browser window was opened in the session; afterwards `true` for a card outside the results, for an id no card has and for the string `"1"`, each leaving no row selected.

## G5. `guiSelectedNotes()` (R1783)

- No parameters. Not a collection action.
- When Browse is showing: the note ids of the selected rows, each once, in the order the rows appear in Browse from top to bottom (in cards mode a note whose several cards are selected appears at its first selected row). `[]` when nothing is selected.
- When Browse is not showing: `[]`.
- The interaction only reads; the window does not change. The reference's order is not defined by its interface: in notes mode it follows its table's selection, in cards mode it is the order its database returns the distinct notes of the selected cards. Meguri's row order is the rule (transcripts compare as a set, `servers/transcripts` T11 `compare.set`).

## G6. `guiAddCards(note?)` (R1784, R1785)

| Parameter | Type | Default |
|---|---|---|
| `note` | object; `null` counts as absent | absent: open the Add panel as it is |
| `note.deckName` | string, required when `note` is present | |
| `note.modelName` | string, required when `note` is present | |
| `note.fields` | object, field name → string | `{}` |
| `note.tags` | list of strings | `[]` |
| `note.audio`, `note.video`, `note.picture` | `null`, an attachment object or a list of them (`servers/ankiconnect-media` X10) | none |
| `note.options` | object | `{}` |
| `note.options.closeAfterAdding` | boolean | `false` |

Other members of `note` and of `note.options` (for example `allowDuplicate`) are ignored.

Member errors (envelope E12.2, checked before the collection wait, in this order; the first is the reply): `missing parameter: note.deckName`, `missing parameter: note.modelName`; `invalid parameter: note.deckName`, `invalid parameter: note.modelName`; `invalid parameter: note.fields` when `fields` is not an object or one of its values is not a string; `invalid parameter: note.tags` when `tags` is not a list or one of its elements is not a string; `invalid parameter: note.options` when it is not an object; `invalid parameter: note.options.closeAfterAdding` when it is not a boolean. An attachment's own member errors are `servers/ankiconnect-media` X10.2's.

Core work, in this order, before any interaction:

1. The deck named `deckName` must exist, else `deck was not found: <deckName>`. Then the note type named `modelName`, else `model was not found: <modelName>`. Deck first, as in the reference. Names are matched as `addNote` matches them (`servers/ankiconnect-actions`).
2. `fields` members are matched to the note type's fields by exact name, case included; members that name no field are ignored. (Difference from `addNote`, whose matching ignores case; the reference does the same.) Fields not given are empty.
3. Attachments are stored and appended to the fields they name, exactly as for `addNote` (`servers/ankiconnect-media` X10.2, R1761, R1762): an attachment that fails writes its escaped error text into the fields it lists and does not fail the call; an attachment that fails and has no `fields` member fails the call with `missing parameter: fields` (X10.2 step 3), and then nothing is stored. A successful attachment without `fields` appends nothing. The stored files are the action's one `LocalApi` op of the media store (envelope E14, R1790), committed before the interaction is posted. They stay in the media folder whatever follows: the app's `refused`, the 10 s timeout, Keep draft, or the user never adding the note; Check Media reports them as unused.
4. Nothing else is written: the request does not change the current deck, the current note type or the Add screen's remembered defaults (U27). Those follow R801 when the user adds the note from the panel.

The interaction then asks the app to present the in-window Add panel (never a new window):

| Add panel state | What the app does | Answer |
|---|---|---|
| closed, or open without unsaved draft | shows the panel prefilled with deck, note type, fields (with the attachment references) and tags | `presented` |
| holds an unsaved draft (U195) | keeps the draft and shows "An external tool sent a new card — Replace draft / Keep draft" (R1785) | `presented` |
| an unsaved sheet is open | nothing | `refused` |

- Replace draft loads the prefill as in the first row; Keep draft drops the prefill and leaves the draft untouched. The choice happens after the reply.
- With `closeAfterAdding: true` the panel closes after the user's next successful add from that prefilled panel; with Keep draft it has no effect.
- Without `note`, the panel is presented as it is (its draft, if any, untouched).
- The reply is `0` once the app answers `presented`, in every case above (the note is not saved, R1784). The reference also replies with the unsaved note's id, which is `0` (observed with a note, without `note` and with `"note": null`, each within about 0.2 s of the request, so it does not wait for the user; a second `guiAddCards` while its Add window held text also replied `0` at once).

## G7. `guiEditNote(note)` (R1786)

| Parameter | Type |
|---|---|
| `note` | integer, required |

- When no note has id `note`: `note was not found: <note>` (decimal id), without navigating. Difference: the reference replies with Anki's own not-found message there (observed: Anki's database-inconsistency message, which names the id in quotes).
- Otherwise the interaction asks the app to show Browse on the search `nid:<note>`, select the first row of its results (the note's only row in notes mode) and open the editor on the note. Answer `done` → reply `null`. Difference: under Anki 26.9.3 the reference fails here; its edit window cannot be built and the reply is an error envelope with a runtime text (observed), so `null` is Meguri's reply from R1786 (G11).

| Params | Reply |
|---|---|
| `{"note": 999}` (no such note) | error `note was not found: 999` |
| `{"note": <id of n1>}` | `null`; Browse searches `nid:<id>`, the row is selected, the editor shows n1 |
| `{"note": "1"}` | error `invalid parameter: note` |

## G8. Host interactions

The kinds the core posts for this file (`servers/host-security` H11.2), their payload and the answers it accepts. Each is resolved exactly once. An answer the kind does not accept (another answer kind, a wrong shape) is not accepted: the resolve returns `false`, the interaction stays open and, without a valid answer, ends at its timeout (H11.2).

| Kind | Payload | Answers | Timeout |
|---|---|---|---|
| `GuiBrowse` | search text or none | `done`, `refused` | 10 s |
| `GuiSelectCard` | card id | `selected{bool}`, `refused` | 10 s |
| `GuiSelectedNotes` | none | `notes{ids}`, `refused` | 10 s |
| `GuiAddCards` | none, or deck id, note type id, field texts by field name, tags, `closeAfterAdding` | `presented`, `refused` | 10 s |
| `GuiEditNote` | note id | `done`, `refused` | 10 s |
| `SyncNow` | none | `result{exchange result or unavailable reason}` (G9) | none (G9) |
| `BrowseNote` | the note id as the decimal digit string received (1 to 19 ASCII digits, possibly beyond the 64-bit range) | `done`, `refused` (G10) | 10 s, reply not waiting |

Payloads and answers cross the host channel as JSON text (`servers/host-security` H11.2, H20). Their forms, which the Swift handler (U146), the router (U144) and the test host (`servers/transcripts` T7.5) all use:

| Kind | Payload JSON | Answer JSON |
|---|---|---|
| `GuiBrowse` | `{"query": <string or null>}` | `"done"`, `"refused"` |
| `GuiSelectCard` | `{"card": <card id>}` | `{"selected": <bool>}`, `"refused"` |
| `GuiSelectedNotes` | `{}` | `{"notes": [<note id>, …]}` (G5's order), `"refused"` |
| `GuiAddCards` | `{"note": null}`, or `{"note": {"deck_id": <id>, "notetype_id": <id>, "fields": {<field name>: <text>, …}, "tags": [<tag>, …], "close_after_adding": <bool>}}` (fields in field order, after G6 step 3) | `"presented"`, `"refused"` |
| `GuiEditNote` | `{"note": <note id>}` | `"done"`, `"refused"` |
| `SyncNow` | `{}` | `{"outcome": <name>}`, with `<name>` one of G9's outcomes `succeeded`, `incomplete`, `waiting`, `failed`, `cancelled`, `unavailable`; plus `"class": <error class name or null>` for `waiting` and `failed`, and `"reason": <reason name>` for `unavailable` (`collectionUnavailable`, `notConnected`, `reviewActive`, `paused`, `iCloudUnavailable`, `decisionRequired`, `recoveryRequired`). Other members of the `sync/policy` P12.6 result are not sent |
| `BrowseNote` | `{"nid": "<digits>"}` | `"done"`, `"refused"` |

Ids in payloads and answers are JSON integers. Example: `{"outcome": "unavailable", "reason": "notConnected"}` gives `sync: auth not configured`; `{"outcome": "waiting", "class": null}` gives `sync: waiting`.

- The sort of `guiBrowse` is not in the payload: the core has written it (G3) and Browse reads it from the collection.
- Answers carry no window state back other than what the reply needs. An interaction that ends because the server is turned off or the app quits ends with its timeout outcome (H11.2, H10.4): `cannot navigate: Meguri did not respond` for the GUI kinds, G9's `cancelled` for `SyncNow`.

## G9. `sync` (R1754)

- No parameters; members of `params` are ignored (envelope E12.2), so Yomitan's `{"version": 2}` runs the action (the reference fails it, G11). Mutating and a collection action (envelope E12.3): it is refused in read-only mode and first waits up to 30 s for the collection, then gets `collection is not available` (R1797).
- The core raises `SyncNow`; the app makes the request `SyncNowRequested{source: ankiConnect}` (`sync/policy#P3`, `#P12`). The availability check, the exchange and its result are exactly Sync Now's (`sync/policy#P12.1` to `#P12.6`). A request while an exchange runs joins it. The app shows nothing for this action (`sync/status-strings#T6`).
- It is the routine iCloud exchange only. It never starts an Anki transfer of any kind (R1549, `sync/policy#P21`), and it does not wait for media: P12.3 step 7 continues after the reply.
- The request waits as long as the exchange runs; `SyncNow` has no timeout. Every exchange ends with one P12.6 result, which the app gives as the answer, and an unavailable check answers at once, so the request is answered in every case (envelope E14): through the answer, or through the core's `cancelled` when the server is turned off or the app quits (G8). The exchange has no overall time limit, but each of its steps either progresses or reaches a bound (`sync/policy#P12.5`). When the client closes the connection meanwhile, the exchange goes on; the reply is dropped.

| Outcome | Reply |
|---|---|
| `succeeded{…}` (held-back, newer-pending and waiting-incoming counts and media state do not matter) | `null` |
| unavailable `collectionUnavailable` | error `collection is not available` |
| unavailable `notConnected` | error `sync: auth not configured` |
| unavailable `reviewActive` | error `sync: not available during a review session` |
| unavailable `paused` | error `sync: paused` |
| unavailable `iCloudUnavailable` | error `sync: iCloud is not available` |
| unavailable `decisionRequired` | error `sync: a decision is needed in Meguri` |
| unavailable `recoveryRequired` | error `sync: iCloud sync needs attention in Meguri` |
| `incomplete{…}` | error `sync: incomplete` |
| `waiting{class, …}` | error `sync: waiting (<class>)` |
| `waiting` with class none (`sync/policy#P12.3` step 3, P15) | error `sync: waiting` |
| `failed{class}` | error `sync: failed (<class>)` |
| `cancelled` | error `sync: cancelled` |

- `<class>` is the error class name exactly as `sync/policy` writes it (`throttled`, `quotaExceeded`, …).
- `sync: auth not configured` is the text the reference gives when no sync account is set up, kept so that clients treat Meguri's "iCloud not connected" the same way. The other `sync:` texts are Meguri's.

## G10. Migaku `/create` `searchNote` (R1733)

- The Migaku server's legacy `/create` route parses the command and gives its reply (`servers/migaku-received-log`, U140). For `searchNote` with a value of 1 to 19 ASCII digits it posts `BrowseNote` with that digit string, unchanged, and replies without waiting for the answer. Other values never reach this file (that spec's 400 reply).
- The app navigates as `guiEditNote` does without opening the editor: Browse on `nid:<digits>` with the first row selected, under G2 rules 3 to 6 (one at a time with the GUI kinds, an unsaved sheet or an Add panel draft leaves everything unchanged, bring to front applies, a review session is left as by the user). An id no note has, including a value beyond the 64-bit range (which `nid:` accepts, `search/grammar` G8), still shows Browse on `nid:<digits>`, with no rows.
- `searchNote` is neither an AnkiConnect action nor gated by the AnkiConnect settings (key, read-only, allow-list).

## G11. Differences from AnkiConnect

Each needs a `conformance/deviations.yaml` entry once transcripts exist (U136); window behavior is not recorded and needs none.

| Case | Reference | Meguri | Req |
|---|---|---|---|
| where GUI actions show | separate browser, Add and Edit windows | the main window's Browse and in-window Add panel | R1782, R1784, R1786 |
| `guiBrowse` with a bad `reorderCards` | the browser opens and searches, then the error is replied (observed: the selection that follows uses the new search's rows) | the error is replied and nothing changes | R1782 |
| `guiBrowse` with a non-string `query` | error with a runtime text, after the browser opened | `invalid parameter: query` | R1782 |
| `guiBrowse` with a query that fails to parse | the browser opens and shows a modal warning; request handling stops until it is dismissed; then the error is replied | the error is replied at once and nothing changes | R1782, R1791 |
| texts of `reorderCards should be a dict:`, `invalid card order:`, `invalid columnId:` with a value that is `-0` or is neither a string nor an integer in plain digits | the parsed value in its language runtime's text form (`None`, `True`, `0` for `-0`, `100.0` for `1e2`) | the value as compact JSON (G2) | R1782 |
| `guiBrowse` with a column not sortable in the mode | a modal notice; request handling stops until it is dismissed | no notice; sort unchanged | R1782, R1791 |
| `guiBrowse` new sort column whose first direction is descending | the requested `ascending` is replaced by descending | the requested direction is used | R1782 |
| `guiBrowse` sort write | the browser's own sort setting (a write the user cannot undo) | a `LocalApi` op that leaves the undo history (G3); refused in read-only mode | R1796, R1779 |
| `guiSelectCard` with a card not in the results | clears the selection; `true` while the browser window exists | selection unchanged; `false` | R1783 |
| `guiSelectCard` with a non-integer `card` (`"1"`) | `true` while the browser window exists | `invalid parameter: card` | R1783 |
| `guiSelectedNotes` order | the table's selection order (notes mode) or the database's (cards mode) | Browse row order | R1783 |
| `sync` with `params` members (Yomitan sends `{"version": 2}`) | error (unexpected keyword argument) | ignored; the action runs | R1740 |
| `guiAddCards` with an open Add window | the reply comes at once (observed); when the window's fields hold text, a non-blocking question offers to discard the text or keep editing, and the window is replaced only when the user discards; an empty window is replaced at once | Replace / Keep prompt, draft kept unless replaced | R1785 |
| `guiAddCards` side effects | selects the deck and sets the current note type at once | none until the user adds | U27 |
| `closeAfterAdding` | ignored | honoured | R1784 |
| `guiEditNote` unknown id | Anki's not-found message | `note was not found: <id>` | R1786 |
| `guiEditNote` existing note | fails under Anki 26.9.3: error envelope with a runtime text | Browse on `nid:<id>`, editor open; `null` | R1786 |
| unsaved work | not checked | `cannot navigate: close the open sheet first` | R1787 |
| `sync` | Anki's own sync with its server; also starts a GUI sync | the iCloud exchange; never an Anki transfer | R1754, R1549 |
| GUI actions not in G1 | implemented | `unsupported action` (envelope E12.4) | R1775 |

## G12. Scenarios

"Rec." rows compare the reply with the reference recording (`servers/transcripts` T11), with the G11 entries named as T12 transforms; "Meguri" rows are declared scenarios (T13, with the T7.3 steps) or are asserted by `core/crates/meguri-servers/tests/ankiconnect_gui.rs` (with the `TestHost` scripting answers) or by `app/MeguriUITests/Integrations/GuiActionsUITests.swift` (real HTTP to 127.0.0.1:8765, window state read by accessibility ids). Requests use `version` 6. Collection and `activeCols` as in G3's examples unless stated (the recorder's profile stores the same `activeCols`).

| Id | Setup and request | Reply | Kind |
|---|---|---|---|
| `browse-ids` | `guiBrowse {"query": "deck:German"}` | `{"result": [c1, c2], "error": null}` | Rec. |
| `browse-reorder` | `guiBrowse {"query": "deck:German", "reorderCards": {"order": "descending", "columnId": "noteCrt"}}` | `[c1, c2]`; UI: Browse sorted by Created descending; `sortType` `noteCrt`, `sortBackwards` true | Rec. reply, Meguri window |
| `browse-reorder-undo` | an undoable edit; then `browse-reorder` | the edit is still undoable; redo history unchanged | Meguri (G11) |
| `browse-reorder-read-only` | read-only on; `browse-reorder`'s request | `Meguri: the API is in read-only mode`; sort keys unchanged | Meguri (G11) |
| `browse-reorder-default-cols` | `activeCols` absent (defaults); `browse-reorder`'s request | `invalid columnId: noteCrt` | Rec. |
| `browse-null-params` | `guiBrowse {"query": null, "reorderCards": null}` | `[]`; sort unchanged | Rec. |
| `browse-no-query` | `guiBrowse {}` | `[]` | Rec. |
| `browse-bad-order` | `reorderCards: {"order": "up", "columnId": "noteCrt"}` | `invalid card order: up` | Rec. |
| `browse-missing-member` | `reorderCards: {"order": "ascending"}` | `Must provide a "columnId" and a "order" property"` | Rec. |
| `browse-hidden-column` | `reorderCards: {"order": "ascending", "columnId": "cardLapses"}` | `invalid columnId: cardLapses` | Rec. |
| `browse-unknown-column` | `reorderCards: {"order": "ascending", "columnId": "nope"}` | `invalid columnId: nope` | Rec. |
| `browse-order-before-column` | `reorderCards: {"order": "up", "columnId": "nope"}` | `invalid card order: up` | Rec. |
| `browse-order-number` | `reorderCards: {"order": 5, "columnId": "noteCrt"}` | `invalid card order: 5` | Rec. |
| `browse-order-null` | `reorderCards: {"order": null, "columnId": "noteCrt"}` | `invalid card order: null` | Meguri (G11, text; the reference writes `None`) |
| `browse-unsortable` | `reorderCards: {"order": "ascending", "columnId": "question"}` with Question displayed | the ids; sort unchanged; no alert | Meguri (the reference blocks on a modal notice, G11) |
| `browse-bad-query` | `guiBrowse {"query": "("}` | the Browse error text for `(` | Meguri (window unchanged; the reference blocks on a modal warning, G11) |
| `browse-dict-value` | `guiBrowse {"query": "x", "reorderCards": ["noteCrt"]}` | `reorderCards should be a dict: ["noteCrt"]` | Meguri (G11, text) |
| `select-no-browse` | Browse not showing; `guiSelectCard {"card": c1}` | `false` | Rec. (reference: no browser window yet in the session) |
| `select-in-results` | after `browse-ids`; `guiSelectCard {"card": c2}` | `true`; row c2 selected | Meguri |
| `select-not-in-results` | after `browse-ids`, row c1 selected; `guiSelectCard {"card": c3}` | `false`; row c1 still selected | Meguri (G11) |
| `selected-none` | Browse not showing; `guiSelectedNotes` | `[]` | Rec. |
| `selected-rows` | Browse on `deck:German`, rows c1 and c2 selected; `guiSelectedNotes` | `[n1, n2]` | Meguri |
| `add-unknown-deck` | `guiAddCards {"note": {"deckName": "Nope", "modelName": "Basic", "fields": {}}}` | `deck was not found: Nope` | Rec. |
| `add-unknown-model` | deck `Default`, `modelName: "Nope"` | `model was not found: Nope` | Rec. |
| `add-deck-first` | both unknown | `deck was not found: Nope` | Rec. |
| `add-empty-panel` | Add panel closed; valid note with fields, tags, one `picture` by `data` | `0`; panel shows deck, note type, fields with `<img src="…">`, tags | Rec. reply, Meguri window |
| `add-no-note` | `guiAddCards {}` | `0`; panel shown as it was | Rec. reply |
| `add-null-note` | `guiAddCards {"note": null}` | `0`; panel shown as it was | Rec. reply |
| `add-missing-model` | `guiAddCards {"note": {"deckName": "Default"}}` | `missing parameter: note.modelName` | Meguri (text, envelope E18) |
| `add-bad-field-value` | valid note with `fields: {"Front": 1}` | `invalid parameter: note.fields` | Meguri (text, envelope E18) |
| `add-refused-keeps-media` | unsaved deck-options sheet open; valid note with one `picture` by `data` | `cannot navigate: close the open sheet first`; the picture file is in the media folder | Meguri |
| `add-draft-keep` | panel holds unsaved text; valid note; user picks Keep draft | `0`; draft unchanged | Meguri |
| `add-draft-replace` | as above; Replace draft | `0`; panel holds the prefill | Meguri |
| `add-close-after` | `options.closeAfterAdding: true`; user adds | panel closes after the add | Meguri |
| `add-no-config-write` | valid note | `col` config `curDeck`, `curModel` and the Add defaults unchanged | Meguri (G11) |
| `edit-unknown` | `guiEditNote {"note": 999}` | `note was not found: 999` | Meguri (G11) |
| `edit-valid` | `guiEditNote {"note": n1}` | `null`; Browse on `nid:n1`, row selected, editor on n1 | Meguri (the reference fails under Anki 26.9.3, G11) |
| `sheet-refuses` | unsaved deck-options sheet open; each of the five GUI actions | `cannot navigate: close the open sheet first`; sheet unchanged | Meguri |
| `add-draft-refuses-browse` | Add panel holds unsaved text; `guiBrowse {"query": "deck:German"}` | `cannot navigate: close the open sheet first`; draft unchanged | Meguri |
| `no-answer` | `TestHost` never answers `GuiBrowse` | after 10 s, `cannot navigate: Meguri did not respond`; an interaction cancelled event | Meguri |
| `queued-timeout` | `TestHost` holds a `GuiBrowse` without an answer; a second `guiBrowse` posted 4 s later | both time out 10 s after their own posting | Meguri |
| `wrong-answer-kind` | `TestHost` answers `GuiBrowse` with `presented`, then `done` | the first resolve returns `false`; the reply is the ids | Meguri |
| `review-left` | review session on deck German; `guiBrowse {"query": "deck:German"}`; back to the deck | session ended as by the user's switch; resumes normally | Meguri |
| `front-on` | window minimized; bring to front on; `guiBrowse` | window restored and active | Meguri |
| `front-off` | window minimized; bring to front off; `guiBrowse` | window state unchanged; reply as usual | Meguri |
| `front-off-closed` | main window closed; bring to front off; `guiBrowse`; then `guiSelectCard {"card": c1}` | the window opens on Browse, Meguri not active; then `true` | Meguri |
| `select-no-front` | window minimized; `guiSelectCard` | window state unchanged | Meguri |
| `sync-not-connected` | iCloud not connected; `sync` | `{"result": null, "error": "sync: auth not configured"}` | Rec. (reference without a sync login) |
| `sync-ok` | cloud fake lane, connected; `sync` | `null` after the exchange ends `succeeded`; zero Anki-server requests | Meguri |
| `sync-review` | review session active; `sync` | `sync: not available during a review session` | Meguri |
| `sync-read-only` | read-only on; `sync` | `Meguri: the API is in read-only mode` | Meguri |
| `sync-joins` | an exchange running; `sync` | the running exchange's result; no second exchange | Meguri |
| `sync-yomitan-params` | iCloud not connected; `sync` with `params: {"version": 2}` | `sync: auth not configured` | Meguri (G11) |
| `sync-waiting-no-class` | the exchange ends `waiting` with class none | `sync: waiting` | Meguri |
| `search-note` | Migaku `/create` `searchNote` for n1 | Migaku reply per `servers/migaku-received-log`; Browse on `nid:n1` | Meguri |
| `search-note-huge` | Migaku `/create` `searchNote=9999999999999999999` | Migaku `OK`; Browse on `nid:9999999999999999999`, no rows | Meguri |

## Open questions (not normative)

Probe results against the reference are recorded with each question. A question about what Meguri should do stays open with what the probe showed.

- G2 rule 5 lets only the navigating actions bring Meguri to front; R1787 says "GUI actions". `guiSelectCard` and `guiSelectedNotes` do not change the window in the reference either. Confirm. Window activation cannot be observed in the offscreen oracle. Still open.
- G2 rule 4 refuses `guiSelectedNotes` and `guiSelectCard` too while an unsaved sheet is open (U146's test "any GUI action"), although they would not discard anything. Confirm. The reference checks nothing of the kind, so no probe applies. Still open.
- G6: the reply is sent when the panel or the Replace/Keep prompt is presented, before the user chooses; R1784 fixes the value (`0`) but not the moment. Observed: the reference replies `0` as soon as its Add window is shown, without waiting for the user. Still open (policy).
- G6: a `deckName` naming a filtered deck. The reference opens its Add window on it (the add then fails there). This draft presents the panel and leaves the refusal to the Add panel's own rules (U195); `addNote` refuses filtered decks at once (R1758). Decide whether `guiAddCards` should refuse with `addNote`'s filtered-deck error instead. Not probed: the v6 action set cannot create a filtered deck, so the recorder needs a setup step for it. Still open.
- G9: the `sync:` error texts other than `sync: auth not configured` are this draft's; no source fixes them. Observed: without a sync login the reference replies `sync: auth not configured`, and with `params` `{"version": 2}` it fails with its unexpected-keyword text. Still open (accept the texts). G10: the `/create` `searchNote` request shape is `servers/migaku-received-log`'s; this file takes its 1 to 19 digit value as the payload.
- G9: `SyncNow` has no timeout, relying on every exchange ending with a P12.6 result. Decide whether a safety limit (and its error text) is wanted for an app that never answers, given that `sync/policy` P12.5 sets no overall time limit on the exchange.
- G2 rule 5: with bring to front off, a closed main window is opened without activating Meguri. Confirm, or keep the window closed and refuse the action.
- G2 rule 6: what the screen shows when a navigating action ends a review session (for example whether N11.6's session summary appears) belongs to U150/U175's exit, which no spec defines yet. `review-left` cannot be finalized until it does.
- G2 note: should a navigating action that arrives during a review session still switch to Browse (asbplayer's refresh option sends two `guiBrowse` after every note update)? R1788 says it leaves the session; the Product Contract owner may prefer to leave the review in place and update Browse in the background. `servers/ankiconnect-clients` should list the asbplayer call.
