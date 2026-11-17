# AnkiConnect client call inventory

Spec: servers/ankiconnect-clients
Requirements: R1945; supports R1734 to R1778 (the calls listed here), R1679, R1771, R1799, R1944
Anki: 26.09.3 (through the AnkiConnect add-on; collection behavior through the specs U141 to U143 cite)
Scenarios: conformance/recordings/servers/ankiconnect/clients/ (`servers/transcripts` T2). C7 lists them as request tables.

The AnkiConnect calls that the clients Meguri must keep working make, so that each call shape gets at least one recorded transcript (R1945). Only action names, parameter shapes, envelope traits and call orders are listed; no client code, and no content from the user's collection, sessions or scripts. Every example value is synthetic.

Interface targets: AnkiConnect API version 6, as served by the add-on build of 2025-11-09 (the transcript oracle's pin, `servers/transcripts` T9). The clients are Yomitan, asbplayer and mpvacious (versions not pinned, C9.1), the user's AI-agent sessions and scripts, and card templates that call AnkiConnect from inside a card.

What is known per client is the set of actions it calls and its envelope traits (C2, C3). The query forms, media forms and call orders that a client uses are known only where C3 says so; everything else in C3 and C4 is an assumption, marked as such and listed in C9. The scenarios still record every listed shape from the add-on, so the transcripts hold whichever form a client turns out to use.

What each action does in Meguri is defined by `servers/ankiconnect-envelope` (U141: envelope, origins, key, `requestPermission`, `apiReflect`, `multi`), `servers/ankiconnect-actions` (U142: collection actions), `servers/ankiconnect-media` (U143) and `servers/ankiconnect-gui` (U146: GUI actions and `sync`). Error texts that the add-on takes from its language runtime are compared by shape only (`servers/ankiconnect-envelope#E18`, `servers/transcripts` T12). This file only says which calls must be covered.

## C1. Notation

- A call is written `action(params)` with the `params` members and their JSON types: `string`, `int`, `bool`, `[T]` for a list, `{…}` for an object, `?` for an optional member, `A | B` for either form.
- `note` objects (`addNote`, `canAddNotes`, `guiAddCards` and the like) have the shape **N**: `{deckName: string, modelName: string, fields: {<field name>: string}, tags?: [string], options?: O, audio?: M | [M], video?: M | [M], picture?: M | [M]}`.
- **O** (duplicate options): `{allowDuplicate?: bool, duplicateScope?: string, duplicateScopeOptions?: {deckName?: string | null, checkChildren?: bool, checkAllModels?: bool}}`.
- **M** (media attachment): `{filename: string, data?: string (base64), path?: string, url?: string, skipHash?: string, deleteExisting?: bool, fields?: [string]}`. Several content members may be given; the first truthy one of `data`, `path`, `url`, in that order, is used, and falsy members (`""`, `0`, `false`, `null`, `[]`, `{}`) count as absent (`servers/ankiconnect-media` X1, X3.1). Clients send `deleteExisting` as a boolean, but any value is accepted and its truthiness counts (X4, X10.1).
- **E** is the request envelope `{action: string, version: int, params?: {…}, key?: string}`.
- "bare" means the reply is the result itself (request `version` 4 or less); "wrapped" means `{"result": …, "error": null}` (version 5 or more). Errors are `{"result": null, "error": "<message>"}` whatever the version.

## C2. Clients and their envelope traits

| Client | How it connects | `Origin` | `version` sent | `key` | Traits |
|---|---|---|---|---|---|
| Yomitan (dictionary popup, browser extension) | browser `fetch` to `http://127.0.0.1:8765` | its extension origin (`chrome-extension://<id>` or `moz-extension://<id>`) | 2, so successes are bare | only when the user set a key | probes available actions with `apiReflect`; batches `findNotes` in `multi` |
| asbplayer (subtitle player, browser extension) | browser `fetch` | its extension origin | 6, wrapped | optional | calls `requestPermission`; uses `createDeck` and `createModel` |
| mpvacious (mpv script in Lua) | `curl` from the player process | none | 6, wrapped | optional | writes audio and screenshots straight into the folder `getMediaDirPath` returns, then references the file names in fields |
| AI-agent sessions and scripts (Claude Code, the user's own scripts) | `curl` or short scripts | none | 6 assumed (C9.2) | none | read-only exploration of decks, note types, notes and cards |
| card templates calling AnkiConnect from a card (for example JPMN's `POST http://127.0.0.1:8765`) | the card webview | the card webview's origin (C9.6) | unknown | none | must fail fast and be refused by default (R1679) |

- Extension origins pass the add-on's default list because it holds `http://localhost`, which also admits every `chrome-extension://`, `moz-extension://` and `safari-web-extension://` origin (R1743). No client in this inventory needs an entry added to the list.

## C3. Call inventory

Each row is one call shape. "Used by": Y = Yomitan, A = asbplayer, M = mpvacious, G = agent sessions and scripts; a client is listed when it calls the action. "Form" says whether the client's use of this particular form (query, media source, optional members) is known or assumed (C9). The reply members named are the add-on's reply.

| # | Call | Used by | Form | Reply members |
|---|---|---|---|---|
| 1 | `version()` | Y (v2), A, G | known | the integer 6 |
| 2 | `requestPermission()` | A | known | `permission`, `requireApikey`, `version` (note the lower-case `k` in `requireApikey`: that is the member name the add-on sends) |
| 3 | `apiReflect(scopes: ["actions"], actions: [string] \| null)` | Y | known | `actions`: the subset that exists |
| 4 | `multi(actions: [E])`, sub-actions `findNotes(query)` | Y | known; sub-action `version` assumed (C9.3) | one reply per sub-action, each bare or wrapped by its own `version` |
| 5 | `deckNames()` | Y, A, G | known | list of full deck names with `::` |
| 6 | `modelNames()` | Y, A, G | known | list of note type names |
| 7 | `modelFieldNames(modelName: string)` | Y, A | known | field names in order |
| 8 | `findModelsByName(modelNames: [string])` | M | known | full note type objects (legacy dict shape) |
| 9 | `findNotes(query: string)` with a deck and field query (`"deck:<name>" "<field>:<value>"`) | Y, A | assumed (C9.8) | note ids |
| 10 | `findNotes(query)` with `nid:<id>` | Y, A, G | assumed (C9.8) | note ids |
| 11 | `findNotes(query)` with `added:1` | M | known | note ids |
| 12 | `findNotes(query)` with `"note:<name>"` and `is:new` | G | assumed (C9.8) | note ids |
| 13 | `findCards(query: string)` | Y, A, G | query forms assumed (C9.8) | card ids |
| 14 | `notesInfo(notes: [int])` | Y, A, M, G | known | `noteId`, `modelName`, `tags`, `fields{<name>: {value, order}}`, `cards`, `mod`, `profile` |
| 15 | `notesInfo(query: string)` | G | assumed (C9.8) | as 14 |
| 16 | `notesModTime(notes: [int])` | A | known | `[{noteId, mod}]` |
| 17 | `cardsInfo(cards: [int])` | Y, A | known | `cardId`, `note`, `deckName`, `modelName`, `fields`, `fieldOrder`, `question`, `answer`, `css`, `ord`, `type`, `queue`, `due`, `interval`, `factor`, `reps`, `lapses`, `left`, `mod`, `nextReviews`, `flags` |
| 18 | `cardsModTime(cards: [int])` | A | known | `[{cardId, mod}]` |
| 19 | `areSuspended(cards: [int])` | A | known | `[bool \| null]` |
| 20 | `suspend(cards: [int])` | Y | known | bool, by `servers/ankiconnect-actions#A12.1` (a one-card list: whether it changed) |
| 21 | `canAddNotes(notes: [N])` | Y | known | `[bool]` |
| 22 | `canAddNotesWithErrorDetail(notes: [N])` | Y | known | `[{canAdd: true} \| {canAdd: false, error: string}]` |
| 23 | `addNote(note: N)` with `options` O | Y, A, M | `options` assumed (C9.5) | the new note id, or an error string |
| 24 | `addNote(note: N)` with `audio` and `picture` M items using `url` | none known | add-on parameter (C9.5) | the new note id; failed attachments become escaped text in the field (R1762) |
| 25 | `addNote(note: N)` with `picture` M using `data` | A | assumed (C9.5) | the new note id |
| 26 | `addNote(note: N)` whose fields reference files the client wrote into the media folder | M | known | the new note id |
| 27 | `addNotes(notes: [N])` | Y | known | `[int]`, or one error listing every failing note (R1760) |
| 28 | `updateNoteFields(note: {id: int, fields: {…}, audio?: M \| [M], picture?: M \| [M]})` | Y, A, M | media members assumed (C9.5) | null |
| 29 | `addTags(notes: [int], tags: string)` (space-separated) | A, M | known | null |
| 30 | `storeMediaFile(filename: string, data: string)` | Y, A | known for Y (base64 `data`); assumed for A (C9.5) | the stored file name |
| 31 | `getMediaDirPath()` | M | known | an absolute folder path that exists and is writable |
| 32 | `changeDeck(cards: [int], deck: string)` | M | known | null |
| 33 | `createDeck(deck: string)` | A | known | the deck id |
| 34 | `createModel(modelName: string, inOrderFields: [string], css?: string, isCloze?: bool, cardTemplates: [{Name?: string, Front: string, Back: string}])` | A | optional members assumed (C9.5) | the new note type object |
| 35 | `guiBrowse(query: string, reorderCards?: {order: "ascending" \| "descending", columnId: string})` | Y, A, M | `query` known (Y with `nid:` queries); `reorderCards` is an add-on parameter no client is known to send | card ids |
| 36 | `guiEditNote(note: int)` | Y | known | null (R1786; the pinned build fails instead, C9.11) |
| 37 | `guiAddCards(note: N)` | A, M | known | the unsaved note's id, `0` (R1784; the add-on also returns `0`) |
| 38 | `guiSelectedNotes()` | M | known | note ids |
| 39 | `sync()` | Y | known | null, or an error |
| 40 | `storeMediaFile(filename, data, deleteExisting: false)` | reported for A (C9.10) | unverified | the stored file name, or the renamed name on a conflict |
| 41 | `addNote(note: N)` with `audio` M using `data` | reported for A (C9.10) | unverified | the new note id |
| 42 | `createModel` without `isCloze` and with templates that have no `Name` | reported for A (C9.10) | unverified | the new note type object (templates named by the add-on) |
| 43 | `updateNoteFields(note: {id, deckName, modelName, fields, tags, options, audio?, picture?})`: a whole N plus `id` | reported for A (C9.10) | unverified | null |
| 44 | `findNotes(query)` with `added:2`, and with `"<field>:*<word>*" OR "<field>:*<word2>*"` | reported for A (C9.10) | unverified | note ids |
| 45 | `findCards(query)` with `(edited:<n> OR rated:<n>) (<query>)`, `prop:due<=<n>` and `"<field>:_*"` | reported for A (C9.10) | unverified | card ids |
| 46 | `findNotes(query)` with a union `(<q1>) or (<q2>)`, and inside `multi` with `nid:<a>,<b> (<q>)` | reported for Y (C9.10) | unverified | note ids |
| 47 | `guiBrowse(query: "nid:<a>,<b>")` | reported for Y and A (C9.10) | unverified | card ids |

Errors the clients key on (R1738) appear in rows 23, 27 and 22: `cannot create note because it is a duplicate`, `cannot create note because it is empty`, `model was not found: <name>`, `deck was not found: <name>`; and on every row: `unsupported action`, `valid api key must be provided`, `collection is not available`.

## C4. Call sequences

Which actions each client calls is known (C3); the order in which it calls them is not (C9.9). The sequences below are assumed orders built from those action sets. Each is covered by one multi-exchange scenario (C7), so that state carried from one call to the next (a new note id, a stored file name, a created deck) is exercised; they become client facts only once C9.9 is settled.

- **Yomitan, card creation** (assumed): `version` → `apiReflect` with its action names → `deckNames`, `modelNames`, `modelFieldNames` → `multi` of `findNotes` for duplicate checks → `canAddNotesWithErrorDetail` → `storeMediaFile` (base64 audio) → `addNote` referencing the stored name → `guiBrowse` with `nid:<new id>`.
- **Yomitan, overwrite** (assumed): `findNotes` → `notesInfo` → `updateNoteFields` → `notesInfo`.
- **asbplayer, first run** (assumed): `version` → `requestPermission` → `deckNames` → `createDeck` → `modelNames` → `createModel` → `modelFieldNames`.
- **asbplayer, update** (assumed): `findNotes` with `added:1` → `notesInfo` → `storeMediaFile` → `updateNoteFields` → `addTags` → `guiBrowse` with `nid:<id>`.
- **mpvacious, add** (assumed): `getMediaDirPath` → (the client writes an audio file and an image into that folder) → `findModelsByName` → `addNote` with `[sound:<file>]` and `<img src="<file>">` in fields → `findNotes` with `added:1` → `notesInfo` → `changeDeck`.
- **mpvacious, update last card** (assumed): `guiBrowse` → `guiSelectedNotes` → `notesInfo` → (writes a file) → `updateNoteFields` → `addTags`.
- **agent exploration** (assumed): `version` → `deckNames` → `modelNames` → `findNotes` → `notesInfo` → `findCards` → `notesInfo(query)`.

In the mpvacious sequences, the file written into the media folder must be collection media by the time `addNote` or `updateNoteFields` returns (R1771), so a later `retrieveMediaFile` or a `media_dir` observation (`conformance/observations` O4) finds it.

## C5. Coverage rules

- Every row of C3 has at least one scenario in C7. Rows that clients use with several `version` values (row 1) have one scenario per value.
- Scenarios are `source: oracle` (recorded from the add-on) unless C7 says `declared`. A declared scenario is used only where the add-on cannot be recorded: Meguri-only behavior (R1679's card origin refusal, R1771's folder pickup) or where no oracle state exists.
- Every scenario carries the tag `client:yomitan`, `client:asbplayer`, `client:mpvacious`, `client:agent` or `client:card`; a scenario for an add-on parameter that no client is known to send (C3 row 24) carries `client:none`. `tools/mg servers-contract --tag client:<name>` runs one client's set (`servers/transcripts` T16). Every oracle scenario whose requests carry no `Origin` (env M and G) also carries the tag `no-origin` (`servers/transcripts` T12); declared scenarios do not, since no deviation entry applies to them (T12).
- Scenarios that change the collection end with an `observe` step so the stored effect is compared too, not only the reply (R1948 for AnkiConnect is covered this way): `{db_dump: {tables: [notes, cards]}}`, with `decks` or `tags` added where the calls create decks or tags that the scenario checks, and `media_dir` (the conformance name of the media folder listing, `conformance/observations` O4) where they store or reference media. In C7, `db_dump` stands for that table list and `media_dir` for the folder listing; each scenario file gives its exact list. A bare `db_dump` is never used: its `notetypes`, `fields` and `templates` rows are the stock note types' (R1959), and `col` and `config` hold the add-on's id-named keys (`servers/transcripts` T10.6).
- A scenario's requests carry the client's envelope traits of C2: the `Origin` header (a synthetic extension id: `chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa` for Yomitan, `chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb` for asbplayer), the `version` and no `key` unless the scenario sets one.
- Requests of env Y and A send `Content-Type: application/json`; env M and G send no `Content-Type`. Which header each client really sends is open (C9.4); since the server ignores `Content-Type` (R1734), `yomitan/content-type-variants` checks that the reply does not depend on it. A scenario per server rule already covers the preflight (U141).
- A call whose parameters C7 does not spell out uses the parameters of the single-call scenario named in brackets, with entity tokens taken from the scenario's own earlier replies.

## C6. Common setup

The C7 scenarios share one synthetic setup (`conformance/dsl` D6), stated once here and repeated in each file:

- decks `Lang` and `Lang::Mining`;
- note type `Vocab` (standard, fields `Word`, `Reading`, `Meaning`, `Audio`, `Picture`, one template `Recognition` with front `{{Word}}` and back `{{FrontSide}}<hr id=answer>{{Meaning}}{{Audio}}{{Picture}}`, CSS `.card {font-size: 20px;}`);
- notes `#n1` (`Word` `alpha`, `Meaning` `first`, deck `Lang::Mining`, tags `mined`) and `#n2` (`Word` `beta`, `Meaning` `second`, deck `Lang`, no tags), each with one card (`#c1`, `#c2`);
- media file `old.mp3` (16 bytes of `0x00`);
- no stored Browse columns, so the displayed card columns are the defaults `noteFld`, `template`, `cardDue`, `deck` (`browser/config-keys` K4.1).

Media bytes in requests are synthetic: a 4-byte `data` payload (`AAECAw==`) for audio and the 67-byte 1×1 PNG of `conformance/fixtures/servers/ankiconnect/pixel.png` for pictures. `url` sources point at the scenario's HTTP fixtures (`conformance/dsl` D6.9) through `${fixture_base}` (`servers/transcripts` T10.2), on each executor's own loopback, never the internet.

## C7. Planned scenarios

All ids are under `servers/ankiconnect/clients/`. "env Y" means Yomitan's envelope traits (C2, C5); likewise A, M, G. GUI calls in oracle scenarios are recorded with Anki's windows on the offscreen platform; Meguri's executor answers their interactions with the test defaults of `servers/transcripts` T7.5 unless the row scripts one.

Every single-call oracle row except `asbplayer/request-permission-key`, and each call of the sequence rows on its own, was probed on the pinned add-on on 2026-11-10 with the C6 setup made through the add-on's own actions (provenance in `oracle/provenance/servers-ankiconnect-clients.md`). The expected replies below are the observed ones; where the first draft differed, the row now says what was observed.

The oracle rows are recorded under `conformance/recordings/servers/ankiconnect/clients/` (`servers/transcripts` T2), each twice with byte-identical goldens (`servers/transcripts` T16), except `none/add-note-url-media`, which is not written yet: it waits for HTTP fixtures (`conformance/dsl` D6.9, still a draft) and the recorder's `${fixture_base}`. The declared rows (`mpvacious/media-dir-pickup`, `card/refused-by-default`, `card/allowed-when-listed`) are written under the same folder. A row that names an A18 or G11 difference is replayed with that `conformance/deviations.yaml` entry, whose `scenarios` list then names the client scenario and whose pointer is the step the row gives; where the row gives Meguri's value, that value is the entry's `replace`.

| Scenario | Rows | Requests (env, then calls in order) | Expected (oracle unless declared) |
|---|---|---|---|
| `yomitan/version-v2` | 1 | env Y: `version` | recorded; bare `6` |
| `asbplayer/version-v6` | 1 | env A: `version` | recorded; `{"result": 6, "error": null}` |
| `agent/version-default` | 1 | env G but no `version` member: `{"action":"version"}` | recorded; bare `6` (version defaults to 4) |
| `asbplayer/request-permission` | 2 | env A: `requestPermission` | recorded; `{"permission":"granted","requireApikey":false,"version":6}` wrapped |
| `asbplayer/request-permission-key` | 2 | `server_config.api_key: "k1"`; env A: `requestPermission` without key, then `deckNames` without key, then `deckNames` with `key: "k1"` | recorded; `requireApikey` true; `valid api key must be provided`; the deck list |
| `yomitan/api-reflect` | 3 | env Y: `apiReflect(scopes: ["actions"], actions: [every action name of rows 1 to 39, plus "notAnAction"])` | recorded; bare `{"scopes":["actions"],"actions":[…]}` listing every name except `notAnAction`, in the order given |
| `yomitan/multi-find-notes` | 4 | env Y: `multi(actions: [{action: findNotes, params: {query: "\"deck:Lang::Mining\" \"Word:alpha\""}}, {action: findNotes, version: 2, params: {query: "Word:gamma"}}, {action: findNotes, params: {}}])` | recorded; bare list of three replies: `["#n1"]`, `[]`, `[]` (observed: `findNotes` without `query` is `[]`, `servers/ankiconnect-actions#A9.1`; a sub-action that lacks a required parameter, such as `modelFieldNames` with `{}`, gives the runtime's missing-argument text, compared by shape) |
| `yomitan/content-type-variants` | 5 | env Y: `deckNames` three times, with `Content-Type: application/json`, with `Content-Type: text/plain;charset=UTF-8`, and with none | recorded; the same bare deck list each time |
| `yomitan/deck-model-field-names` | 5, 6, 7 | env Y: `deckNames`, `modelNames`, `modelFieldNames(modelName: "Vocab")`, `modelFieldNames(modelName: "Nope")` | recorded; bare lists; the last the error `model was not found: Nope`, the same text on both sides (observed) |
| `mpvacious/find-models-by-name` | 8 | env M: `findModelsByName(modelNames: ["Vocab"])` | recorded; one object with `id` `#m7` (the stock note types are `#m1` to `#m6`, `servers/transcripts` T10.1), `flds`/`tmpls` ids `#rand`, `mod` `@…`, and `did` `null` on both sides: the recorder applies the C6 setup through Anki's library (`servers/transcripts` T5), which leaves the note type's `did` as stored (none), and Meguri never sets it from an add (`servers/ankiconnect-actions` A18 `ankiconnect-add-defaults`). Observed: when the C6 notes were instead added through the add-on's own `addNote`, its reply carried the id of the deck last added into; a golden with a deck id there was not recorded from this setup and is recorded again |
| `yomitan/find-notes-deck-field` | 9 | env Y: `findNotes(query: "\"deck:Lang\" \"Word:alpha\"")`; then with `deck:Lang::Mining` | recorded; `["#n1"]` both (deck search includes children) |
| `agent/find-notes-nid` | 10 | env G: `findNotes(query: "nid:${#n2}")` | recorded; `{"result":["#n2"],"error":null}` |
| `mpvacious/find-notes-added` | 11 | env M: `addNote(note: {deckName: "Lang", modelName: "Vocab", fields: {Word: "delta", Meaning: "fourth"}})`, then `findNotes(query: "added:1")` | recorded; `#n3`; `["#n1","#n2","#n3"]` (setup notes count as added today: the clock is frozen at `clock.start`) |
| `agent/find-notes-note-is-new` | 12 | env G: `findNotes(query: "\"note:Vocab\" is:new")` | recorded; `["#n1","#n2"]` |
| `agent/find-cards` | 13 | env G: `findCards(query: "deck:Lang")`, `findCards(query: "(")` | recorded; the golden is `["#c2","#c1"]` (scan order, observed), Meguri's `["#c1","#c2"]` (`servers/ankiconnect-actions` A18 `ankiconnect-search-order`, at `/steps/0/reply/body/json/result`); then the search error (R1778). Observed: `Invalid search: `, then U+2068, Anki's message for an unclosed bracket, U+2069. Meguri's error is the `unclosed_group` text of `search/errors#E7` exactly, with no prefix and no U+2068 or U+2069 (the error envelope `{"result": null, "error": <text>}` is the same on both sides). The isolation marks are not normalized in error strings (`servers/transcripts` T10.1), so `ankiconnect-search-error-text` replaces the whole string at `/steps/1/reply/body/json/error` with Meguri's text |
| `agent/notes-info-ids` | 14 | env G: `notesInfo(notes: ["#n1", 1000000000001])` | recorded; one full object and `{}` for the unknown id |
| `agent/notes-info-query` | 15 | env G: `notesInfo(query: "tag:mined")` | recorded; `#n1`'s object |
| `asbplayer/notes-cards-mod-time` | 16, 18 | env A: `notesModTime(notes: ["#n1"])`, `cardsModTime(cards: ["#c1"])` | recorded; `[{"noteId":"#n1","mod":"@0"}]`, `[{"cardId":"#c1","mod":"@0"}]` |
| `asbplayer/cards-info` | 17 | env A: `cardsInfo(cards: ["#c1", 1000000000001])` | recorded; one object (`question`/`answer` normalized, `servers/transcripts` T10.3) and `{}` |
| `asbplayer/are-suspended` | 19 | env A: `areSuspended(cards: ["#c1", 1000000000001])` | recorded; `[false, null]` |
| `yomitan/suspend` | 20 | env Y: `suspend(cards: ["#c2"])` twice; `observe: [db_dump]` | recorded; `true` then `false`; card queue −1 |
| `yomitan/can-add-notes` | 21, 22 | env Y: `canAddNotes` and `canAddNotesWithErrorDetail`, each with the same three N, all `{deckName: "Lang", modelName: "Vocab"}`: fields `{Word: "alpha"}` (a duplicate of `#n1`), `{Word: "gamma"}`, and `{Word: ""}` | recorded; `[false, true, false]`; `[{canAdd:false,error:"cannot create note because it is a duplicate"},{canAdd:true},{canAdd:false,error:"cannot create note because it is empty"}]` |
| `yomitan/add-note-options` | 23 | env Y: `addNote` N `{deckName: "Lang::Mining", modelName: "Vocab", fields: {word: "gamma", Meaning: "third"}, tags: ["y"], options: {allowDuplicate: false, duplicateScope: "deck", duplicateScopeOptions: {deckName: null, checkChildren: false, checkAllModels: false}}}`; the same again; then with `allowDuplicate: true`; `observe: [db_dump]` | recorded; a note id; the duplicate error; a second note id (field `word` matched case-insensitively, R1758) |
| `none/add-note-url-media` | 24 | HTTP fixtures `/a.mp3` (4 bytes, 200) and `/missing.png` (404); env Y traits: `addNote(note: {deckName: "Lang", modelName: "Vocab", fields: {Word: "url"}, audio: {url: "${fixture_base}/a.mp3", filename: "y_a.mp3", fields: ["Audio"]}, picture: [{url: "${fixture_base}/missing.png", filename: "y_p.png", fields: ["Picture"]}]})`; `observe: [db_dump, media_dir]` | recorded; a note id; `Audio` ends with `[sound:y_a.mp3]`; `Picture` holds the escaped failure text, normalized `<fixture>/missing.png download failed with return code 404` (`servers/transcripts` T10.3; Meguri's text is the same, `servers/ankiconnect-media` X3.4); `y_a.mp3` stored |
| `asbplayer/add-note-data-picture` | 25 | env A: `addNote(note: {deckName: "Lang", modelName: "Vocab", fields: {Word: "shot"}, picture: {data: <pixel.png base64>, filename: "shot.png", fields: ["Picture"]}})`; `observe: [db_dump, media_dir]` | recorded; `Picture` ends with `<img src="shot.png">` |
| `mpvacious/add-note-media-dir` | 26, 31 | env M: `getMediaDirPath`; `media_dir_write {name: "mpv_a.mp3", base64: "AAECAw=="}`; `media_dir_write {name: "mpv_s.png", fixture: <pixel.png>}`; `addNote(note: {deckName: "Lang", modelName: "Vocab", fields: {Word: "mpv", Audio: "[sound:mpv_a.mp3]", Picture: "<img src=\"mpv_s.png\">"}})`; `observe: [db_dump, media_dir]` | recorded; `"<media-dir>"`; a note id; both files in the collection's media (R1771) |
| `mpvacious/media-dir-pickup` | 26, 31 | declared; env M: `getMediaDirPath`; `media_dir_grant {}` (`servers/transcripts` T7.3: scans of a folder never granted are suspended, T20); the two `media_dir_write` steps above (the picture's bytes inlined as `base64`); `media_dir_scan` twice; `observe: [media_dir]` | `"<media-dir>"`; the grant `{baseline: 0}` (the folder is empty when granted, so neither file is `baseline`, which scans never copy, X11.2); the first scan `{status: done, rescan: true, ingested: 0, notices: []}` (both files `pending`); the second `{status: done, rescan: false, ingested: 2, notices: []}`; both files in the collection's media (`servers/ankiconnect-media` X11.3). The 2 s pickup time of R1771 is the app watcher's and is checked by U144's tests |
| `yomitan/add-notes-all-or-nothing` | 27 | env Y: `addNotes(notes: [{deckName: "Lang", modelName: "Vocab", fields: {Word: "eta"}}, {deckName: "Lang", modelName: "Nope", fields: {Word: "theta"}}])`; `observe: [{db_dump: {tables: [notes, cards]}}]` | recorded; the joined error; no note added (R1760). The add-on adds the valid note and then deletes it, which leaves graves and a changed `col`; those tables are not observed here, and the difference is `servers/ankiconnect-actions`'s deviation `ankiconnect-addnotes-rollback`, asserted by its own scenario |
| `yomitan/update-note-fields` | 28 | env Y: `findNotes(query: "Word:alpha")`, `notesInfo(notes: ["#n1"])`, `updateNoteFields(note: {id: "#n1", fields: {Meaning: "changed", nope: "x"}, audio: {data: "AAECAw==", filename: "u.mp3", fields: ["Audio"]}})`, `notesInfo(notes: ["#n1"])` | recorded; `["#n1"]`; the object; null; `Meaning` changed, `Audio` ends with `[sound:u.mp3]`, `mod` `@0` |
| `mpvacious/update-last-card` | 28, 29, 35, 38 | env M: `guiBrowse(query: "nid:${#n2}")`; `interaction {kind: GuiSelectedNotes, answer: {notes: ["#n2"]}}`; `guiSelectedNotes`; `updateNoteFields(note: {id: "#n2", fields: {Picture: "<img src=\"mpv_u.png\">"}})`; `addTags(notes: ["#n2"], tags: "mpv clip")` | recorded; `["#c2"]`; `["#n2"]` (the scripted answer follows the recording, `servers/transcripts` T7.5); null; null; `#n2` gains the tags `clip` and `mpv`. Observed: `guiSelectedNotes` right after `guiBrowse(query: "nid:${#n2}")` can return the browser's earlier selection (`["#n1"]` after a previous browse, `[]` in a fresh profile), because the add-on replies before Anki runs the search; the recorder waits for that search before each exchange (`servers/transcripts` T16, C9.12) |
| `asbplayer/add-tags` | 29 | env A: `addTags(notes: ["#n1", "#n2"], tags: "asb  extra")`; `observe: [db_dump]` | recorded; null |
| `yomitan/store-media-file` | 30 | env Y: `storeMediaFile(filename: "y_store.mp3", data: "AAECAw==")`, the same with `data: "BAUGBw=="`, then with `filename: "old.mp3"` | recorded; `"y_store.mp3"`, `"y_store.mp3"`, `"old.mp3"`: each given name, the file replaced (R1769: `deleteExisting` defaults true; observed) |
| `asbplayer/store-media-keep-existing` | 40 | env A: `storeMediaFile(filename: "old.mp3", data: "AAECAw==", deleteExisting: false)`; `observe: [media_dir]` | recorded; `old.mp3` unchanged, and the new bytes stored under the returned name `old-<40 lower-case hex digits>.mp3` (observed: the digits are the SHA-1 of the new bytes, `old-a02a05b025b928c039cf1ae7e8ee04e7c190c0db.mp3` for `AAECAw==`; with bytes equal to the existing file's, the reply is `old.mp3` and nothing is written) |
| `asbplayer/add-note-data-audio` | 41 | env A: `addNote(note: {deckName: "Lang", modelName: "Vocab", fields: {Word: "clip"}, audio: {data: "AAECAw==", filename: "asb.mp3", fields: ["Audio"]}})`; `observe: [db_dump, media_dir]` | recorded; `Audio` ends with `[sound:asb.mp3]` |
| `asbplayer/create-model-minimal` | 42 | env A: `createModel(modelName: "Clip2", inOrderFields: ["Sentence"], cardTemplates: [{Front: "{{Sentence}}", Back: "{{FrontSide}}"}])` | recorded; the note type object, its template named `Card 1` and its CSS Anki's stock CSS (observed; Meguri's CSS is `servers/ankiconnect-actions` A18 `ankiconnect-createmodel-css`) |
| `asbplayer/update-note-fields-whole-note` | 43 | env A: `updateNoteFields(note: {id: "#n2", deckName: "Lang", modelName: "Vocab", fields: {Meaning: "whole"}, tags: ["x"], options: {allowDuplicate: true}})`; `notesInfo(notes: ["#n2"])`; `observe: [db_dump]` | recorded; null; `Meaning` changed, tags unchanged (the extra members are ignored) |
| `asbplayer/find-forms` | 44, 45 | env A: `findNotes(query: "added:2")`; `findNotes(query: "\"Word:*alp*\" OR \"Word:*bet*\"")`; `findCards(query: "(edited:1 OR rated:1) (deck:Lang)")`; `findCards(query: "prop:due<=1")`; `findCards(query: "\"Picture:_*\"")` | recorded; each result list (observed: card lists in scan order, `ankiconnect-search-order`; `prop:due<=1` is `[]` while every card is new) |
| `yomitan/find-union-and-nid-filter` | 46 | env Y: `findNotes(query: "(\"Word:alpha\") or (\"Word:beta\")")`; `multi(actions: [{action: findNotes, params: {query: "nid:${#n1},${#n2} (deck:Lang::Mining)"}}])` | recorded; `["#n1","#n2"]`; `[["#n1"]]` |
| `yomitan/gui-browse-nid-list` | 47 | env Y: `guiBrowse(query: "nid:${#n1},${#n2}")` | recorded; `["#c1","#c2"]` |
| `mpvacious/change-deck` | 32 | env M: `changeDeck(cards: ["#c2"], deck: "Lang::New")`; `observe: [db_dump]` with `decks` | recorded; null; deck `Lang::New` created and `#c2` in it |
| `asbplayer/first-run` | 33, 34, 7 | env A: `createDeck(deck: "Video::Mining")`, the same again, `createModel(modelName: "Clip", inOrderFields: ["Sentence", "Image"], css: ".card {}", isCloze: false, cardTemplates: [{Name: "Card", Front: "{{Sentence}}", Back: "{{FrontSide}}<hr id=answer>{{Image}}"}])`, `modelFieldNames(modelName: "Clip")` | recorded; deck token twice (same id); the note type object; `["Sentence","Image"]` |
| `yomitan/gui-browse-nid` | 35 | env Y: `guiBrowse(query: "nid:${#n1}")`; then with `reorderCards: {order: "descending", columnId: "noteFld"}`; then with `reorderCards: {order: "descending", columnId: "noteCrt"}` | recorded; `["#c1"]` twice; then the error `invalid columnId: noteCrt`, since `noteCrt` is not a displayed column of C6's setup (`servers/ankiconnect-gui` G3 row 6 gives Meguri the same rule) |
| `yomitan/gui-edit-note` | 36 | env Y: `guiEditNote(note: "#n1")`, `guiEditNote(note: 1000000000001)` | recorded. Observed: the first call fails in the pinned build with a runtime attribute-error text (the add-on's edit dialog does not run on Anki 26.9.3), where Meguri answers null (R1786); the second gives Anki's `Your database appears to be in an inconsistent state. Please use the Check Database action. No such note: '1000000000001'`, where Meguri answers `note was not found: 1000000000001` (`servers/ankiconnect-gui` G7). Both differences are declared in `servers/ankiconnect-gui` G11 (rows `guiEditNote` unknown id and existing note, scenarios `edit-unknown` and `edit-valid`). Pointers: the first is a `replace` of the whole body at `/steps/0/reply/body/json` with `null`, since a success at `version` 2 is bare while the golden holds the error envelope (C1); the second a `replace` at `/steps/1/reply/body/json/error`, both sides keeping the envelope. `1000000000001` is an id that never existed: no entity has it, so neither text gets a token there (`servers/transcripts` T10.1) and both keep the 13 digits. Meguri's executor answers the `GuiEditNote` interaction of the first call with its test default `done` (`servers/transcripts` T7.5); the second raises none (`servers/ankiconnect-gui` G7) |
| `asbplayer/gui-add-cards` | 37 | env A: `guiAddCards(note: {deckName: "Lang", modelName: "Vocab", fields: {Word: "epsilon"}})` | recorded; `0`, the same on both sides (R1784) |
| `yomitan/sync-not-configured` | 39 | env Y: `interaction {kind: SyncNow, answer: {outcome: unavailable, reason: notConnected}}` (`servers/ankiconnect-gui` G8); `sync` | recorded; the error `sync: auth not configured`, the same text on both sides (`servers/ankiconnect-gui` G9, G12 `sync-not-connected`) |
| `agent/exploration` | C4 agent | env G: the agent sequence of C4: `version`, `deckNames`, `modelNames`, `findNotes` [`agent/find-notes-note-is-new`], `notesInfo` on its result, `findCards` [`agent/find-cards`, first call], `notesInfo` [`agent/notes-info-query`] | recorded |
| `yomitan/card-creation` | C4 Yomitan | env Y: `version`; `apiReflect` [`yomitan/api-reflect`]; `deckNames`, `modelNames`, `modelFieldNames(modelName: "Vocab")`; `multi` [`yomitan/multi-find-notes`, first sub-action, with `Word:zeta`]; `canAddNotesWithErrorDetail` with N `{deckName: "Lang", modelName: "Vocab", fields: {Word: "zeta"}}`; `storeMediaFile(filename: "y_zeta.mp3", data: "AAECAw==")`; `addNote` with that N and `Audio: "[sound:<the returned name>]"`; `guiBrowse(query: "nid:<the new id>")`; `observe: [db_dump, media_dir]` | recorded |
| `mpvacious/add` | C4 mpvacious add | env M: `getMediaDirPath`; `media_dir_write {name: "mpv_add.mp3", base64: "AAECAw=="}`; `media_dir_write {name: "mpv_add.png", fixture: <pixel.png>}`; `findModelsByName` [`mpvacious/find-models-by-name`]; `addNote(note: {deckName: "Lang", modelName: "Vocab", fields: {Word: "kappa", Meaning: "tenth", Audio: "[sound:mpv_add.mp3]", Picture: "<img src=\"mpv_add.png\">"}})`; `findNotes(query: "added:1")`; `notesInfo(notes: ["#n3"])`; `changeDeck(cards: ["#c3"], deck: "Lang::Mining")`; `observe: [db_dump, media_dir]` with `decks` | recorded |
| `asbplayer/update` | C4 asbplayer | env A: `findNotes(query: "added:1")`; `notesInfo` on `#n1`; `storeMediaFile(filename: "asb_u.png", data: <pixel.png base64>)`; `updateNoteFields(note: {id: "#n1", fields: {Picture: "<img src=\"<the returned name>\">"}})`; `addTags(notes: ["#n1"], tags: "asb")`; `guiBrowse(query: "nid:${#n1}")`; `observe: [db_dump, media_dir]` | recorded |
| `card/refused-by-default` | R1679 | declared; `POST /` with `Origin: meguri-media://media`, body `{"action":"deckNames","version":6}`; then the same with `Origin: null` | 403, empty body, no CORS header, both |
| `card/allowed-when-listed` | R1679 | declared; `server_config.cors_origins: ["http://localhost", "meguri-media://media"]`; the first request above | 200, the deck list wrapped. Valid only if the card webview sends `meguri-media://media`; if it sends `null`, R1744 refuses it whatever the list holds and this scenario cannot pass (C9.6) |

## C8. Actions no client here uses

The rest of the v1 action set (R1754 to R1774, R1782 to R1788) is covered by the AnkiConnect specs' own scenarios (U141 to U143, U146), not by this inventory. Calls outside the v1 set that a client might make get `unsupported action` (R1775); none of the clients above is known to need one.

## C9. Open questions

Only action names per client and the envelope traits of C2 are established. Until the questions below are settled, the scenarios use the shapes above, which follow the add-on's documented parameters. The probe of 2026-11-10 settled the add-on's replies to these shapes (C7); C9.1 to C9.10 ask what the clients send, which no add-on probe answers, so they stay open.

1. The client versions. Yomitan, asbplayer and mpvacious are not pinned to a release; a newer release may add calls (for example Yomitan's use of `getMediaFilesNames` or `deckNamesAndIds`).
2. The `version` and parameter forms the agent sessions and scripts used. Only their actions are known (`version`, `deckNames`, `modelNames`, `findNotes`, `findCards`, `notesInfo`); the scenarios assume version 6 and add one version-less call (`agent/version-default`).
3. Whether Yomitan's `multi` sub-actions carry their own `version` (and their own `key` when a key is set). The scenario covers both forms.
4. Which request headers each client sends: `Content-Type` (`application/json`, `text/plain` or none), and for mpvacious whether `curl` sends form encoding. A review of Yomitan's source reported `application/json` with a CORS-mode `fetch`; that is not verified here. R1734 makes the server ignore `Content-Type`, so only the preflight behavior of browsers depends on it (U141).
5. The exact media attachment forms Yomitan and asbplayer use in `addNote` and `updateNoteFields` (`url` against `data`, `skipHash` values, `deleteExisting`), beyond "storeMediaFile with base64 data" for Yomitan; whether `addNote` callers send `options`; and which optional `createModel` members asbplayer sends.
6. The `Origin` that Meguri's card webview sends for a `fetch` or `XMLHttpRequest` from a card: `meguri-media://media` (the document origin of `render/media-serving`) or `null` if WebKit treats the custom scheme as opaque. Both are refused by default. If it is `null`, R1679's opt-in ("add the card origin to the AnkiConnect allow-list") cannot work, because R1744 and `servers/ankiconnect-envelope` E7.2 row 3 refuse `null` whatever the list holds, `*` included; the two requirements then conflict and the product owner must choose.
7. Which actions card templates such as JPMN call; only a `POST` to `127.0.0.1:8765` that shows an error popup when it fails is known.
8. The query forms behind rows 9, 10, 12, 13 and 15: which clients send a deck-and-field query, an `nid:` query to `findNotes`, `"note:<name>" is:new`, and `notesInfo(query)`. The scenarios record each form from the add-on either way.
9. The order in which each client calls its actions (C4). Only the action sets are known; the C4 sequences are assumptions.
10. Rows 40 to 47 are forms that a review of the asbplayer and Yomitan sources reported (media `data` for asbplayer's audio and pictures, `deleteExisting: false`, `createModel` without `isCloze` or template names, `updateNoteFields` with a whole note, the listed search forms, Yomitan's lower-cased first field name in duplicate queries, its query union and `nid:` filter inside `multi`, asbplayer's export through `addNote` with tags rather than `addTags`). They are not verified against a pinned client release; they are covered because they are valid add-on parameters and cost one scenario each.
11. (Settled, 2026-11-10.) `guiEditNote` on an existing note fails in the pinned build with a runtime attribute-error text, and on an unknown id gives Anki's inconsistent-database text (C7 `yomitan/gui-edit-note`). Meguri keeps R1786's replies; `servers/ankiconnect-gui` G11 already declares both differences, with its scenarios `edit-unknown` and `edit-valid`.
12. (Settled for the recorder, 2026-11-10.) `guiSelectedNotes` after `guiBrowse` returned the browser's earlier selection, not the browsed note: the add-on replies to `guiBrowse` before Anki runs the search, so a fast `guiSelectedNotes` read the selection from before it (`[]` in 1 recording of 8 in a fresh profile). The recorder now waits before each exchange until the Browse window has run the search its box shows (`servers/transcripts` T16), and `mpvacious/update-last-card` records `["#n2"]` in every run. A client that calls `guiSelectedNotes` right after `guiBrowse` on the add-on can still see the earlier selection; Meguri's reply is the scripted answer (`servers/transcripts` T7.5).
