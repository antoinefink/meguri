# Search sessions and result sets

Spec: browser/result-sets
Requirements: R1080, R1083, R1084, R1116, R1117, R1142, R1154, R1155, R1156, R1241, R1242, R1244, R1851; exposes R1776–R1778; supports R1106, R1109–R1111, R1113, R1152, R1153, R1215
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/modes/ (not yet recorded: H16)

How the core runs searches for Browse and the local servers, and what it hands back: a search session, its requests and events, the result handle with its ordered ids, row pages, reverse order and resorting, invalidation by change sets, the Cards/Notes toggle, selection by id and index, and the one-shot id search. Which rows a query returns is `search/query-model`; their order is `browser/sorting`; the cells of a row are `browser/columns`. Most of this file is Meguri's own contract (U85); the toggle and selection rules of H11 and H12 reproduce what Anki's browser does.

## H1. Terms

- **Session.** One per Browse instance (U210), or per other caller that searches repeatedly (U86's Find and Replace preview). Sessions are independent: nothing one does cancels or releases anything of another.
- **Request.** One run of a search, a resort (H8) or a toggle (H11) in a session. Each has a **request seq**: a per-session counter, 1 for the first request, then strictly increasing.
- **Handle.** The result of a request (H6): an ordered list of ids plus what the core needs to page, reverse, map and invalidate it. Swift addresses rows by index into a handle; the handle is the only owner of result order, positions and removals.
- **Mode.** Cards (ids are card ids) or notes (ids are note ids), as `search/query-model` Q4.
- **Sort.** A column key and a direction (`browser/sorting` S1, S8).
- **Snapshot.** The single read transaction a request reads the collection in. Its **change seq** is the seq of the newest op whose commit the snapshot includes (`engine/op-model` O4 reserves op seqs at commit), 0 when there is none.
- **Lone rows.** Cards whose note row is missing and notes with no card (`search/query-model` Q6).
- **Page.** 200 ids by default.

## H2. Sessions

- `open_session(listener)` returns a session; `listener` is the synchronous callback interface that receives the session's events (H4). `close_session(session)` cancels its in-flight request and releases all its handles.
- Every call of this file may come from any thread except the main thread (CoreBridge lanes, KTD9). Calls that read the collection read on the reader their section names and hold no statement or transaction when they return (KTD9: never across FFI calls).

## H3. Starting a search

`start_search(session, text, mode, sort, cancel)` returns the new request's seq at once. A search worker of its own then runs it:

1. The text is read by `search/grammar` exactly as given. Callers that take the text from the search field apply the search-field rule first (`search/join-and-replace` J2); the core does not.
2. The search context is built (H9).
3. The plan is built for the mode (`search/query-model` Q9) and the sort is checked against the mode (`browser/sorting` S1). Errors come in the order of `specs/conformance/browser-ops.md` B1: a parse error, then `unsortable_column`, then the run errors.
4. The statement runs in one snapshot on the worker's reader, sorted by the request's sort with the final key (`browser/sorting` S8). A plan with slow terms may stream (`search/query-model` Q10); the result is the same.

- **Workers.** Each in-flight request runs on a search worker thread (`engine/jobs` J6, user-initiated QoS) with its own thread pool for per-row work and its own reader (`engine/lifecycle` L11, one per search worker). The query lane stays free for row pages (H7) while a search runs.
- **Superseding.** Starting a request in a session cancels that session's in-flight request: its cancel token is set and the database's interrupt is called on its worker's reader. The caller's own `cancel` token cancels the request too (`engine/jobs` J4).
- **Availability.** A request reads like any reader request (`engine/lifecycle` L3, L4): when no collection is open, or while the generation changes, it fails with the engine's refusal. Searches still run while a replacement holds the exclusive phase before it starts the swap (U210). When a generation change starts (`engine/lifecycle` L8 step 1), and at suspend (U32), every in-flight request is cancelled so its reader comes back at once.

## H4. Events

A session's listener receives these events. Each carries the request seq.

| Event | Members | When |
|---|---|---|
| `FirstPage` | handle, the first page of ids (fewer when the result is shorter, none when it is empty) | the first page is final (Q10 step 4 for streamed requests) |
| `Progress` | `matched` (ids final so far), `scanned` (candidates tested so far), `revision` (the handle revision `matched` is counted in, H10) | streamed requests only, throttled as `engine/jobs` J5 (at most one per 100 ms) |
| `Complete` | handle, `count`, `revision` (the revision `count` is counted in) | the result is whole |
| `Failed` | the error (H14): for a search error its kind, details (`search/errors` E2), message (E7) and range in UTF-16 code units (E5, H15) | the request cannot give a result |
| `Cancelled` | | the request was superseded, its token was set, its session closed, or H3's availability rule stopped it |

- **One request.** Its events are one of: `Failed`; `Cancelled`; or `FirstPage`, any number of `Progress`, then one of `Complete`, `Failed`, `Cancelled`. Exactly one terminal event (`Complete`, `Failed`, `Cancelled`) ends every request. A `Failed` or `Cancelled` after `FirstPage` releases that handle (H6).
- **Order.** Within a session, every event of a request is delivered before any event of a later request: a new request's first event waits until the request it superseded has delivered its terminal event. Cancellation is prompt (H3), so the wait is short.
- **Delivery.** Events are delivered on a core thread, never on the writer thread and never while the core holds a lock; the Swift listener only yields them into an `AsyncStream` (KTD9). Swift also drops every event whose seq is older than the newest request it started (R1084), as a second guard.
- An empty result gives `FirstPage` with no ids, then `Complete` with count 0 (R1110 is the UI's).

## H5. Membership and order

- **Membership** is `search/query-model` Q4–Q7 for the request's mode, read in the request's snapshot with its context. It is the set `search_ids` (H13) returns for the same text, mode and context on the same collection state.
- **Order** is the request's sort (`browser/sorting`), whose final key makes it total. So the same search, mode, sort and context on the same collection state always give the same list, streamed or not, and resorting to the same sort gives it back.
- Sort keys are read in the same snapshot as membership; Due and Retrievability read now and today from the context (`browser/sorting` S10).

## H6. Handles

A handle holds:

| Member | Meaning |
|---|---|
| ids | the ordered list; in cards mode each entry also keeps its card's note id (16 bytes per row), in notes mode the note id alone (8 bytes) |
| mode, sort | as requested; reversing flips the direction (H8) |
| text, context | the search text as given and the context of H9, with the plan's context reads (`search/query-model` Q8) |
| match seq | the change seq of the snapshot whose predicate run gave the membership (kept by resorts, H8) |
| applied seq | the newest change seq the list reflects: the snapshot's, then the newest change set H10 applied |
| generation | the collection generation of the snapshot (`engine/lifecycle` L3) |
| revision | 0 when made; one more at every change of the list (H8 reverse, H10 prune). Rows a request in progress appends to its final prefix change no revision: earlier indexes keep their rows |
| match data | what H11 needs to derive the other mode without a run, when the request kept it |
| state | in progress or complete |

- **Indexes.** Rows are addressed 0 to count − 1. While the request is in progress only the final prefix exists: its length is the last `Progress` `matched` (or the `FirstPage` length), counted in the revision that event names. An event naming a revision older than the handle's current one (H10 pruned since) is ignored; the prefix length `invalidated_rows` returned stands until the next event.
- **Revisions.** Every call that takes indexes also names the revision they were computed against. When it is not the handle's current revision the call fails with `StaleRevision{current}` and does nothing. Decided 2026-10-27 (data safety first): a bulk operation or a selection mapping therefore never acts on rows other than the ones the caller showed.
- **Lifetime.** Per session the core keeps the in-flight request's handle and the two newest complete handles (so the previous results stay usable while a new search runs, R1107, R1108, and H12 can map from them). An older handle is released; a call with a released handle fails with `UnknownHandle`. A handle whose request failed or was cancelled is released at once, a superseded one included.
- **Memory.** A handle holds its ids, its match data (8 bytes per matched card id) and, while in progress, the ids H10 removed; it holds no cells. On a collection of 500k cards and at most as many notes, a handle takes at most about 8 MB: 16 bytes per row in cards mode, 8 bytes per note plus 8 per matched card in notes mode. A session's peak, three handles while a search runs, is therefore about 24 MB, which U245's share of R1851 (150 MB on iPhone at 500k cards) for Browse must hold. The plan's U85 text (8 bytes per id, current plus previous handle) predates this section; this section governs.
- **Generations.** Every call through a handle checks its generation while it holds its reader (`engine/lifecycle` L3); a handle of an older generation fails with `StaleGeneration{planned, current}`. On `CollectionReplaced` the core releases all such handles, so later calls fail with `UnknownHandle`.

## H7. Rows

- `rows(handle, revision, range, columns)` returns the row records of `browser/columns` C3 for the indexes in `range` (at most 1000), in index order, from one short read transaction on the query reader. Every requested column is computed except `question` and `answer`: their cells are returned as deferred.
- `qa_cells(handle, revision, range, which, cancel)` returns the Question cells, the Answer cells or both (`browser/columns` C10) for those indexes, on the query lane, cancellable between rows. The UI asks for them only while those columns are visible (R1242).
- Rows are read from the collection as it is at the call, not from the snapshot: a row that no longer matches shows its current cells (R1155), and a row whose card or note was deleted but not yet pruned (H10) is the row error `deleted` (`browser/columns` C3.1).
- An index outside the handle's final prefix fails the call with `OutOfRange`.

## H8. Reverse and resort

- `reverse(handle, revision)` reverses the list in memory, flips the sort direction and returns the new revision (R1142). The index i of the old revision is index count − 1 − i of the new one; the UI maps its selection, current row and scroll position by that rule. The result equals the opposite-direction query on the same snapshot (`browser/sorting` S8). It needs a complete handle (`NotComplete` otherwise) and reads nothing.
- `resort(session, handle, sort, cancel)` needs a complete handle: on a handle in progress it fails `NotComplete` before it starts anything, and the in-flight request goes on. Decided 2026-10-27: to sort a search that is still running, the UI starts a new search with the text it searched and the new sort (`start_search`), which supersedes the running one (H3) and runs the predicate again, as a search whose result is not whole must. On a complete handle, `resort` starts a request (H3, H4) that sorts the handle's ids, as they are after every prune, by `sort` in a new snapshot, without running the predicate (R1083). Its handle keeps the old match seq, mode, text and context; its applied seq is the new snapshot's. Rows that stopped matching stay (R1155). A card or note that no longer exists when the new snapshot is read is left out.
- R1244's 100 ms budget covers both on 150k rows (U90 measures).

## H9. Search context

The engine builds each request's context (`search/query-model` Q8) when the request starts: now from the clock, today and next-day-at from the day clock (`sched/day-clock`, U40), and the config values Q8 lists, read in the request's snapshot (`deck:current` reads `curDeck`, `search/names-and-ids` N3.4). Swift never builds a context. The context is fixed for the request and kept by its handle.

## H10. Change sets and invalidation

`invalidated_rows(handle, revision, change_sets)` applies change sets (`engine/change-stream` S1) to a handle and says which rows to refetch:

- The caller passes every `Changes` event it received since it started the request, in seq order, including those received before `FirstPage`. Change sets whose seq is at most the handle's applied seq are skipped; the applied seq becomes the newest seq passed.
- **Removed** (R1156): in cards mode the indexes of cards listed `Removed`; in notes mode the indexes of notes listed `Removed`. These indexes are in the revision the call named, ascending. When any row is removed, the handle drops those rows and its revision goes up by one.
- **Refetch**, as indexes of the new revision, ascending:
  - cards mode: cards listed `Updated`, and every card of the handle whose note is listed `Updated`;
  - notes mode: notes listed `Updated`, and the note of every card listed in the change set (any kind) when that note is still in the handle;
  - every row (`all`), in either mode, when the change set lists a note type or a deck, of any kind (names, templates and deck names reach cells of rows the change set does not list).
- **Membership is never re-evaluated** (R1155): added cards and notes never join a handle, and rows that stop matching stay until the next search. A row removed and then restored by undo does not come back until the next search either. Decided 2026-10-27: undo is an ordinary change set here.
- **Handles in progress.** The call applies to a handle in progress too, on its final prefix, so a deleted row leaves at once (R1156). The handle then keeps every id it removed, and until its request completes the worker leaves those ids out of the rows it confirms later (it reads the request's snapshot, which still holds them). The call and the worker's appends to the prefix never interleave: each removal applies to a whole prefix. Change sets that arrived before `FirstPage` are passed in the first call after it.
- The result is the removed indexes, the refetch indexes or `all`, the revision, and for a handle in progress the length of its final prefix in that revision.

## H11. Cards/Notes toggle

`toggle(session, handle, sort, cancel)` starts a request (H3, H4) for the handle's text in the other mode, sorted by `sort` (the other mode's sort, `browser/config-keys` K5), with a context built when it starts (R1116). Its result is exactly what `start_search` gives for that text, mode, sort and context.

- **Complete handles only.** On a handle in progress `toggle` fails `NotComplete` before it starts anything, and the in-flight request goes on. Decided 2026-10-27: the UI then calls `start_search` with the text it searched, the other mode and that mode's sort, which gives the same result as a toggle that runs the predicate; the selection follows H12's rule for a superseded handle.
- **Without a run.** When no op committed after its match seq, and the new context equals the handle's on every value the plan read (`search/query-model` Q8 context reads), the core derives the set from the handle instead of running the predicate (R1080). Only the sort runs.
- **Derived set.** Q5 decides what the other mode tests; the set below equals a run:

  | From | Query scope (Q5) | Set in the other mode |
  |---|---|---|
  | cards | card and note | the distinct notes of the matched cards |
  | cards | card | the distinct notes of the matched cards whose note row exists |
  | cards | note | the distinct notes of the matched cards, plus the notes without cards that the query matches (tested on those notes alone) |
  | notes | note | every card of the matched notes |
  | notes | card, or card and note | the cards of the matching card-and-note pairs, which the request keeps as match data; for card scope plus the cards without a note row that the query matches (tested on those cards alone) |
  | either mode | either | none: the query runs |

  On a collection without lone rows the extra tests have no rows. A notes-mode handle made without match data, and every other case, runs the predicate.
- The selection and the current row move with `map_selection` (H12).

## H12. Selection by id and index

- `index_of(handle, revision, id)` gives the index of an id, or none. Reviewer entry uses it (R1113): U210 falls back to a `cid:` search when it gives none.
- `map_selection(from, from_revision, selection, current, to, to_revision)` maps a selection between two handles of one session (a re-search, R1154, or a toggle, R1116). `selection` is index ranges of `from` or a list of ids; `current` an id or none. It returns ascending, merged index ranges of `to` and the current index or none:

  | From → to | Selected in `to` | Current in `to` |
  |---|---|---|
  | same mode | every selected id that is in `to` | the current id when it is in `to` |
  | cards → notes | the notes of the selected cards that are in `to` | the current card's note when it is in `to` |
  | notes → cards | every card of the selected notes that is in `to` | the current note's first card (lowest card id, `browser/columns` C1) when it is in `to`; otherwise the note's lowest-id card that is in `to` |

  - **Superseded handles.** A new request releases the in-progress handle it supersedes (H6), so `map_selection` from it fails `UnknownHandle`. Decided 2026-10-27: `from` may instead be a mode, with `from_revision` none and `selection` and `current` given as ids the caller holds (ids of rows it fetched); notes for cards are then read in the read transaction too. Selected rows whose ids the caller never fetched drop out of the selection, which never makes an action touch a row the user did not choose.
  - Nothing caps the selection (R1154; Anki keeps at most 500 rows). Notes are looked up for cards from the handles, cards for notes in one read transaction on the query reader.
  - Decided 2026-10-27: when the current note's first card is not in `to`, Meguri makes the note's lowest-id card in `to` current, so the editor stays on the same note. Anki's mapping gives no current row there; its table then makes the first selected row current.
  - Decided 2026-10-27: when `map_selection` gives no current index, Meguri follows Anki: the first selected row of `to` (the lowest selected index) becomes current, and when nothing is selected the first row of `to` (index 0) is selected and made current. When `to` has no rows, nothing is selected and no row is current. `map_selection` still returns none for the current index; U212 applies this rule to its result.
- `resolve_selection(handle, revision, ranges, target)` gives the ids an action works on (R1117), from one read transaction on the query reader:

  | Handle | Target | Ids |
  |---|---|---|
  | cards | cards | the card ids in index order |
  | cards | notes | the distinct note ids, in the order of their first selected card |
  | notes | notes | the note ids in index order |
  | notes | cards | for each selected note in index order, every card of the note (also cards the search did not match), ascending card id; a note without cards adds none |

  U86 and U87 call it inside the core, so large id lists never cross the FFI.
- The result count is the handle's count (R1111); the selection count is U212's.

## H13. One-shot id search

`search_ids(text, mode)` is a blocking call for the local servers (R1776–R1778). It builds the context as H9 does, reads on a server reader (`engine/lifecycle` L11), and returns every matching id in ascending order, or the search error with the same kind, message (`search/errors` E7) and range as `Failed` gives Browse. It keeps no handle. The empty text returns every id of the mode.

## H14. Errors

| Error | Raised by |
|---|---|
| search errors (`search/errors` E1–E3) | H3, H11, H13 |
| `unsortable_column` (`browser/sorting` S1) | H3, H8 resort, H11 |
| `StaleGeneration{planned, current}`, the engine's refusals (`CollectionUnavailable`) | every call that reads, H3 |
| `StaleRevision{current}` | every call that takes a revision |
| `UnknownHandle` | a released handle (H6), a superseded in-progress one included (H12) |
| `OutOfRange` | an index outside the final prefix (H7) |
| `NotComplete` | `reverse`, `resort` and `toggle` on a handle in progress (H8, H11) |
| storage errors | any call that reads |

## H15. Boundary

- Ids cross the FFI as 64-bit integers; ranges of search errors are converted from character offsets (`search/errors` E5) to UTF-16 code units.
- A handle crosses as an opaque value bound to its session; the request seq crosses with every event.

## H16. Tests and scenarios

Handles, revisions, events and invalidation are Meguri's: `core/crates/meguri-engine/tests/browse_handles.rs` and the CoreBridge `SearchSessionTests` cover them from this text (the U85 test scenarios). Anki's part is the set a toggle gives (H11) and the mapping rules (H12), which the oracle sees through its card and note searches. Scenarios for `conformance/scenarios/browser/modes/`. The two toggle scenarios need only the existing unsorted `search` observation (`specs/conformance/search-ops.md` S1). `sorted-both-modes` uses the sorted requests of `specs/conformance/browser-ops.md` B1, a draft that waits for B6 (the driver and schema change for `order`):

| Scenario | Covers |
|---|---|
| `browser/modes/toggle-sets` | H11's table on a sound collection: one query per scope (card, note, card and note, either) in both modes, so the derived set can be checked against each run |
| `browser/modes/toggle-damaged` | H11 on a collection with a card whose note is missing and a note without cards: the plus rows of the table |
| `browser/modes/sorted-both-modes` | H5: one query sorted by its mode's default column in both modes and both directions; the reversed list equals the other direction (H8) |
