# Review session

Spec: sched/review-session
Requirements: R600, R602, R620, R621, R622, R623, R627, R680, R720, R721, R722, R727, R728, R729; core side of R326, R362, R444, R456, R457, R459, R460, R461, R462, R463, R726; AE13 (core part), AE22 (session part), AE27; supports R1807, R1811, R1812, R1813, R1817, R1894, R2006
Anki: 26.09.3 for P1 to P11 (how a held study queue changes between builds); none for C1 to C19 (Meguri's session contract C5, from the plan and the Product Contract)
Scenarios: conformance/scenarios/sched/session/

The review session owns the study queue of one deck while the user studies it. A build (`sched/queue-building`) gives the starting queue; after that, answers, undo and redo change the queue in place, and only the changes of `sched/queue-building#B10` rebuild it. Part P says how the queue changes between builds, as Anki 26.09.3 does it; the oracle checks it (`specs/conformance/session-ops.md`). Part C is Meguri's session contract (C5): the calls the review screen makes, the state tokens that keep an answer from applying to a card that changed, the overlay that keeps the queue ahead of the database, and how the session recovers from failed writes, rollovers and changes made elsewhere. U175 and U176 build on part C.

Terms: `now` is the clock's instant in epoch seconds (milliseconds divided by 1000, rounded down). L is the learn-ahead limit in seconds, `collapseTime` (`sched/config-keys#G2`) as the build read it. `today` and `next_day_at` come from the day clock (`sched/day-clock`).

# Part P. The queue between builds (Anki's behavior)

## P1. The session's queue

A build (`sched/queue-building#B8`) gives the session:

- the **main list**: entries `{card id, kind, card mtime}`, kind `new`, `review` or `interday learning`, in presentation order;
- the **learning list**: one entry `{card id, due, reps, card mtime}` for every intraday learning card the build gathered (queues 1 and 4, `sched/queue-building#B4` phase 1), including those due after now + L;
- the **counts** `{new, learning, review}` (`sched/queue-building#B9`);
- the **learning cutoff** `c`, an instant in epoch seconds: the build instant at first;
- L, the build's day number and `next_day_at`, the load-balancer cache if any (`sched/load-balancer#W2`), and a **build identity**, a value that differs for every build (P9 compares it).

The **edge** is `c` + L.

**Order of the learning list.** Each entry has the key (`r`, `due`), where `r` is 0 when `reps` > 0 and 1 when `reps` is 0; the list is in ascending key order: attempted cards first, then by due time. A build sorts it so (`sched/queue-building#B6`) and a requeued entry is inserted by its key (P5). Only an undo puts an entry back at the front whatever its key (P9).

## P2. What the queue presents (R602, R620)

The queue presents, in this order:

1. the learning entries with `due` ≤ `c`, in list order;
2. the main list, in order;
3. the learning entries with `c` < `due` ≤ `c` + L, in list order.

Learning entries with `due` > `c` + L are **held**: not presented. The **head** is the first presented entry; `head(n)` is the first n. Each entry's reported kind is `learning` for parts 1 and 3 and for interday learning, else `new` or `review` (the `queue` observation's kinds, `specs/conformance/queue-ops.md` Q2).

The cutoff is not the clock. It moves only when a card is answered (P4), when a read finds every count at 0 (P3), and on undo and redo (P9). So a learning card whose due time passes while the user looks at a review card stays in part 3, behind the main list, until the next answer moves the cutoff past it; the answer then brings it to part 1, ahead of the main list (golden `sched/session/requeue/main-not-empty`).

## P3. Reading the queue when every count is 0 (R622)

Every read of the head or the counts first looks at the counts. When new, learning and review are all 0, the cutoff first advances to `now` by P4 step 3's rule; otherwise the read changes nothing.

This is how a learning card that was held becomes available: at the first read at or after its `due` − L. Golden `sched/session/cutoff/refresh-when-empty` (L = 120 s): the only learning card left is due at 10:10:00; reads at 10:07:59 present nothing, a read at 10:08:00 presents it.

## P4. Answering a card from the queue

The answer itself (the card row, the review-log row, deck counters, burying, leech) is `sched/answering`. The queue changes as follows, with the card row the answer writes and the answer instant `t` in epoch seconds:

1. **Remove.** When the card has an entry in the learning list, at any position (presented or held), that entry is removed and learning is lowered by 1, not below 0. Otherwise, when the card is the first entry of the main list, that entry is removed and the count of its kind is lowered by 1 (interday learning lowers learning), not below 0. Otherwise the card is **not at the head**: Anki refuses the answer with an invalid-input error and writes nothing (C9 step 2).
2. **Requeue.** When the row after the answer has `queue` 1 or 4 and `due` < `next_day_at`, a learning entry `{card id, due, reps, mtime}` from that row goes back by P5. Any other row (graduated to review, interday learning, suspended as a leech) leaves the queue.
3. **Advance the cutoff.** With `old` = `c` + L: `c` becomes `t`, and learning grows by the number of learning entries with `old` < `due` ≤ `t` + L. When `t` < `c` (the clock went back), `c` still becomes `t` and nothing is added.
4. **Undo record.** P9 keeps what an undo needs.

Consequences:

- A learning card can be answered from anywhere in the learning list, held or behind the main list; a main-list card only when it is the first of the main list, also when learning cards are presented before it.
- Learning is lowered for any learning entry, also a held one that was not counted, so the learning count can fall below the number of presented learning cards (golden `sched/session/head/learning-anywhere`).
- The requeue's count rule uses the edge from before step 3; step 3 then counts what the new edge takes in, the requeued entry included when it lies between the two edges.

## P5. Where a requeued learning card goes (R621)

With `E` = `c` + L, `c` being the cutoff before P4 step 3:

1. **Not twice in a row.** When the entry's `due` ≤ `E`, the main list is empty and the learning list is not empty: let `f` be the first entry of the learning list in list order (P1). When `f.due` ≥ the entry's `due` and `f.due` + 1 < `E`, the entry's `due` becomes `f.due` + 1. This changes only the queue's entry: the card row keeps the due the answer wrote, and a later build uses the row's due.
2. **Count.** When the entry's `due` (after rule 1) ≤ `E`, learning grows by 1.
3. **Insert** the entry after every entry whose key is less than or equal to its key (P1). Anki's position among entries with an equal key is not defined; scenarios never create equal keys.

| Situation when the answer leaves the card in intraday learning | Result |
|---|---|
| Main list empty; the first learning entry is attempted, due at or after the requeued card and more than 1 s before the edge | the requeued card goes 1 s after it: another card comes first (golden `sched/session/requeue/after-next-learning`, first answer) |
| Main list empty; no other learning card | it is presented again (same golden, last answer) |
| Main list empty; the first learning entry is due earlier than the requeued card | no move: it goes by its due, after that entry |
| Main list empty; the first learning entry's due + 1 is at or past the edge | no move: the requeued card comes first (golden `sched/session/requeue/front-rules`, deck Edge) |
| Main list empty; the first learning entry was never attempted (`reps` 0) | moved to 1 s after it, but its key still sorts it first: it is presented again (same golden, deck Fresh) |
| Main list not empty | no move; it waits in part 3 behind the main list until an answer moves the cutoff past its due (golden `sched/session/requeue/main-not-empty`) |

## P6. Counts after an answer

The counts follow from P4 and P5 alone; no other rule changes them between builds. For an answer at the cutoff's own instant (nothing added by P4 step 3):

| Card answered | Row after the answer | new | learning | review |
|---|---|---|---|---|
| new | intraday learning, due within the edge | −1 | +1 | |
| new | review (Easy, or Good without steps) | −1 | | |
| review | relearning, due within the edge | | +1 | −1 |
| review | review | | | −1 |
| intraday learning | intraday learning, due within the edge | | 0 (−1, +1) | |
| intraday learning | review, or interday learning | | −1 | |
| interday learning | intraday learning, due within the edge | | 0 (−1, +1) | |
| any | suspended as a leech | −1 of its kind | | |

A requeued card due past the edge is not counted until a later cutoff advance takes it in (P4 step 3, P3). Golden `sched/session/drain/mixed-40` checks the counts after every answer of a full drain (R1894).

## P7. Siblings an answer buries

The answer reports the siblings it buried (`sched/sibling-burying#B6`). Meguri removes each one that has a main-list entry, lowering its kind's count by 1. Siblings in intraday learning are never buried. A buried sibling is usually not in the queue at all, since gathering skipped it (`sched/queue-building#B5`); it can be only when the note's cards sit in decks of different presets. Anki leaves such a card in its queue, and its next read of the queue fails when the card comes up (P11).

## P8. Rebuilds (R623)

- `sched/queue-building#B10` classifies each commit using its `ChangeSet`, `ObservedCommit` sequence/generation, card before/after images and session ownership. A rebuild replaces the queue at that instant: the cutoff is the build instant, counts are recomputed, and P5's moves are lost. The current B10 table is authoritative, including whole-collection changes and stored-memory changes.
- Anki rebuilds lazily, at its next read of the queue; Meguri rebuilds when the change commits (C13). The conformance executor rebuilds at the next read, as Anki (`specs/conformance/session-ops.md` E2).
- The session's own answers, and the undo and redo of them, never rebuild (B10); P4 and P9 update the queue instead.

## P9. Undo and redo of an answer (R720, R721, R722)

For each answer the queue keeps: the removed entry as it was (a learning entry with its due and reps, or a main entry with its kind), the requeued entry as inserted (after P5 rule 1), the build identity, and a **snapshot** of (learning count, `c`) taken after P4 steps 1 and 2, before step 3.

**Undo**, when the build identity is still the session's:

1. learning and `c` take the snapshot's values;
2. the requeued entry, when there was one, is removed; when its `due` ≤ the restored `c` + L, learning is lowered by 1, not below 0;
3. the removed entry goes back at the **front** of its list: a learning entry at the front of the learning list whatever its key (learning +1), a main entry at the front of the main list (its kind's count +1).

So the card is presented next, on its question side, and the counts are those from before the answer (R722; golden `sched/session/undo/answer-undo-redo`).

**Redo**, when the build identity is still the session's:

1. the card is removed as P4 step 1 removes it;
2. the requeued entry, when there was one, is inserted again by P5 rule 3, with the due it had;
3. learning and `c` take the snapshot's values again.

After a redo the cutoff is the one from before the answer, not the answer instant: a learning card that the answer had brought into part 1 is back in part 3 (same golden).

**Build changed.** When a rebuild happened between the answer and its undo, Anki drops its queue and builds again at the next read: the undone card takes its place in the fresh build. Meguri builds afresh and then shows the undone card first (C15, R722; P11).

The load-balancer cache follows `sched/load-balancer#W9`: an undo removes the card from it, a redo adds nothing.

## P10. A new day (R459 to R463)

A queue belongs to the day it was built on. Anki notices a new day at its next read of the queue: it drops the queue and the whole undo and redo history, restores buried cards (`sched/day-clock#D7`) and builds again. An answer applied before that read still goes through P4 on the old queue, with the new day's `next_day_at` in P4 step 2; the queue is then dropped anyway. Meguri's order of events is C14; the observable result after the first answer of the new day is the same: a queue built for the new day and an empty history (golden `sched/session/rollover/ae13`).

## P11. Differences from Anki (R1889)

R1889 references: `conformance/dsl#D13`; `conformance/deviations.yaml` header. Proof obligations: P11.1 and C18. This table authorizes no scenario transform.

| Situation | Anki 26.09.3 | Meguri |
|---|---|---|
| The card changed after its buttons were computed (R727, R728) | the answer fails with a "card was modified" error | `Stale`: nothing written, silent rebuild, the notice only when the card is still due (C10) |
| A rebuild while a card is shown (R457, KTD7) | the reviewer moves to the rebuilt queue's head | the shown card stays first while the rebuilt queue still presents it (C4) |
| Undo of an answer after a rebuild (R722) | the card takes its place in the fresh build | the card is shown first (C15) |
| A sibling buried by an answer is still queued (P7) | it stays queued; reading it later fails | it is removed and counted down |
| When a rebuild happens (P8) | at the next read | at the change, in the background |
| A new day while a card is shown (R460, R461) | nothing until the next read; then the queue and history are dropped | the shown card's states and labels are recomputed at once; the rebuild and the history clear follow its answer (C14) |
| Load balancer on and a preset with an Easy Days list of a length other than 0 or 7 (`sched/load-balancer#W2` row 5) | every queue build fails with an error | `startSession` refuses with `PresetUnsupported` naming those presets (C2) |

### P11.1 Production proof mapping

| P11 situation | Mandatory contract assertion |
|---|---|
| R727/R728 | C10 / AE27: Stale, zero writes, rebuilt head and notice predicate |
| R457 | C4 / C13: current-card retention across rebuild |
| R722 | C15 / AE22: undo puts card first after rebuild |
| P7 | C8 / C9: remove and decrement buried queued siblings |
| P8 | C13: background rebuild at observed commit |
| R460/R461 | C14 / AE13: labels now; rebuild/history after answer |
| invalid Easy Days | C2: global check, named preset refusal, zero writes |

C18 owns execution of this mapping. It is separate from scenario activation and adds no transform to D13.

# Part C. The session contract (C5, Meguri)

## C1. Sessions

- A session studies one deck (the deck and its descendants, `sched/queue-building#B2`). The engine has at most one active session per open collection. `startSession` while one is active ends it first (C16).
- A session id is a 64-bit integer, unique for the life of the engine. Every call names its session; a call that names a session that is not the active one fails with `NoSession` and changes nothing.
- All calls run on the interactive lane (KTD9) and are short. Writes go through the executor (`engine/op-model`); the session never writes on the caller's thread.
- Layering (KTD2): the queue (part P), tokens, the overlay and predictions are pure types and functions in `meguri-sched` (`src/session/`); op submission, rendering, events and the journal are in `meguri-engine` (`src/session/`).

## C2. `startSession(deckId, resumeCardId?)`

Result: `Started{sessionId, generation, head, counts, validUntil, timeLim}` or `Refused{PresetUnsupported{presetIds} | FsrsPolicyPending}`.

1. **Admission.** On the writer, atomically check and reserve the collection-owned admission state shared with `enableFsrs` (`sched/fsrs-policy#S3`). No session is exposed before this succeeds. Refusals, in order, write nothing, fire no hook and reserve nothing:
   - policy status is not `ok`: `FsrsPolicyPending`;
   - any card in the selected deck tree has an unsupported home preset under the shared U45 check: `PresetUnsupported` with distinct preset ids ascending. Inspect all represented homes, including suspended, buried, future and otherwise ungathered cards, not only queue entries;
   - balancing is on and any preset of the collection has an Easy Days list of length other than 0 or 7: `PresetUnsupported` with those ids ascending (`sched/load-balancer#W2`). This check is collection-wide, distinct from the home-preset check.
2. **Optimizer.** Fire the session-start hook, which requests cancellation of training without waiting (U47, KTD8).
3. **Selection.** Delegate to `decks/decks#D11`: the existing select-deck op writes only `curDeck`, skip-undo (`engine/undo#M2`), and writes nothing when that value is unchanged. Preserve `activeDecks` here. Complete the reserved start consistently with that write. A failed start releases its reservation and exposes no active session. D11 leaves R41's additional `activeDecks` behavior to U152/U175; those owners must close it for both deck-page opening and review start before final app acceptance, through the shared selection contract with write-on-change and skip-undo preserved.
4. **Build.** Use current U41 queue and U44 cache contracts at the start instant. Build failure releases admission without a partially exposed session.
5. **Current card.** Use `resumeCardId` when presented, otherwise head item 0 (C4).
6. Emit `SessionStarted`; return head(3), counts, C7 `validUntil` and `timeLim`.

An earlier admitted session makes enable refuse with `SessionActive`; an earlier enable completes before admission checks the resulting policy. End releases admission only after C16 drains accepted writes. A caller boolean cannot replace this writer-owned state. Sync-off during an admitted session does not stop its FSRS answers/undo; enable waits until it ends (S3).

The compatibility held queue of `conformance/session-ops#E2` is not this production admission path: its reads write nothing, do not select a deck or fire hooks, and remain valid with FSRS off when no excluded answer is requested.

`startSession` plus `head(3)` fits R1813's 150 ms at 14.4k cards (C18).

## C3. `peekFirstCard(deckId)`

Returns the head item that `startSession(deckId)` would show first, or `Refused` as C2 step 1 would, or none when the queue would present nothing. It writes nothing (no `curDeck`), fires no hook, keeps no queue, and takes at most 20 ms at 150k cards (R1813; C18). U176 renders that card before the session starts; `startSession` returns the same `(cardId, stateToken, renderKey)` when nothing changed in between, and the prepared page is used only when all three match its head item 0.

## C4. Head items, the current card and `head(n)`

**Head item** (no HTML):

| Member | Value |
|---|---|
| `cardId`, `noteId` | ids |
| `kind` | `new`, `learning` or `review` (P2) |
| `stateToken` | C5 |
| `renderKey` | C6 |
| `nextStates` | for Again, Hard, Good and Easy: the label (`sched/answer-labels`) and the delay in seconds (whole days × 86,400 for a delay in days), computed with the session's load-balancer cache (`sched/load-balancer`), so labels show what answering applies (R656) |
| `predictedNext` | for each rating, the card id the queue would present first after that answer, or none (C8) |
| `flag`, `marked` | the card's flag (0 to 7) and whether its note has the `marked` tag (`tags/flags-and-marked`) |
| home-preset values (R330) | from the card's home preset (`sched/presets#V1`, `#V2`): audio (R748), timers (R749), auto advance (R750), maximum answer seconds, leech action |

**Current card.** The current card is item 0 of the last head the session gave the client (`Started`, `Accepted`, `Stale`, `NotAtHead`, `head`, a `QueueHeadChanged` or `CurrentCardChanged` event). After a rebuild, while the rebuilt queue still presents the current card, it stays item 0, ahead of the other presented cards, until it is answered or no longer presented (KTD7, R457). An answer accepts it wherever its entry is (C9 step 2) and removes that entry by P4 step 1's count rule for its list. The conformance executor never has a current card (`specs/conformance/session-ops.md` E2).

**`head(sessionId, n)`** returns `{items, counts, validUntil, terminalState}`: the current card first when the keep rule holds, then the queue's presented order (P2) without it, n items at most (1 to 10). It applies P3 first. When nothing is presented it returns no items, and the session emits `WaitingUntil` or `Finished` (C17). C17.1 defines the terminal state carried by every head.

## C5. `stateToken` (R727)

A digest that changes whenever the states an answer would apply may have changed for a reason other than the passing time of day. Two tokens are compared only for equality. It is a SHA-256 over these parts, each in a fixed encoding:

- the card part: the card row's `type`, `queue`, `due`, `ivl`, `factor`, `reps`, `lapses`, `left`, `odue`, `odid`, `did`, `data` (its exact bytes) and `mod`;
- the home preset's id and its `mtime_secs` (a marker when no preset has that id), the home deck's own desired retention (`sched/presets#V7`; present or absent), and the effective home-input fingerprint below;
- `fsrsShortTermWithStepsEnabled` and `collapseTime` as read (`sched/config-keys#G2`);
- the day number and `next_day_at` (`sched/day-clock`). The next-state engine reads both (`sched/fsrs-transitions#T14`), and `next_day_at` alone moves when the `rollover` hour or the offsets change within a day.

Include the resolved U46 memory state and last-review input when either comes from history rather than stored `data` (`sched/memory-state#M2`, M3). Compute that projection at the head's state-computation instant; changes to its resolved inputs must invalidate the token even without a card-row edit. These parts cover the state inputs of `sched/fsrs-transitions#T14` except passing time. Not in it: the card's flags and `usn`, the note, the load-balancer cache (labels are recomputed after every answer instead, C9 step 5), and the time of day (C7 covers it). Setting a flag (which keeps `mod`, `tags/flags-and-marked`) therefore leaves the token as it was.

| C5 additional input | Required projection |
|---|---|
| `sched/fsrs-transitions#T2.1`, T13 | selected list kind and length; effective parameter f32 bits; support markers |
| T2.5, T11-T14 | effective learning/relearning steps, maximum interval, starting ease, leech threshold, resolved desired retention |
| C12.2 / U44 | bounded answer-time cap, sibling-bury modes, leech action |
| read semantics | existing provider bounds/defaults |
| encoding | typed values; distinct sequence/presence boundaries; exact effective f32 bits |
| same-second changes | equal id/mtime does not imply equal fingerprint when effective inputs differ |

This ADDS to the identity fields above. Preset name, `usn`, opaque/unknown members, optimizer metadata, unused SM-2 settings and unselected older lists are not added dependencies; later unsupported-list markers remain required. If an existing required mtime changes, the before-acceptance token can still change with equal effective semantics. C12.2's after-acceptance guard remains distinct: unrelated mtime/opaque changes alone do not veto an accepted intent. Keep exact card `data`/`mod` requirements; this fingerprint neither rewrites protected bytes nor replaces the full-row precondition.

The **card-row digest** used in preconditions is a different value: `RowDigest` over the whole stored row (`engine/op-model#O7`). The session reads it when it checks a token (C9 step 3) and passes it with the answer.

## C6. `renderKey` and `renderHead`

- `renderKey`: a digest of the card id, `ord`, `did`, `odid`, the card's flags, the note's `mod`, the note type's id and `mtime_secs`, plus the render-input fingerprint below. A page rendered for one key stays valid while the key is unchanged. A prepared page stays valid only while `(cardId, stateToken, renderKey)` is unchanged (KTD7).
- `renderHead(sessionId, renderKeys)` returns one U62 `RenderedCard` per key, with media references for preloading (R444). It runs on the interactive lane. Rendered cards are cached by `renderKey`; the cache keeps at least the last 8 keys, so head diffs and an undo reuse them. A test-only counter counts renders.
- `answer` never renders: its result holds ids, tokens, labels, predictions, counts and flags only.

The extra C6 projection is the consumed review read-set of `render/card-render-pipeline#K2`-K4, K9-K11 and `render/type-answer#A1`, A4:

| Contract boundary | Captured values |
|---|---|
| K2 / K3 | note-type id/name/kind/CSS; selected template ordinal/name and exact question/answer text; deck presence and resolved full name, using the original deck where required |
| K3 / type-answer A1, A4 | field values in order; declared field names/ordinals in order; font/size; tag tokens in stored order with duplicates retained |
| K9 | effective `hideAudioPlayButtons` and `reviewExtra` |
| K11 | render mode and host platform |

All pre-existing C6 card identity, ordinal, deck ids, whole flags and note/type timestamps remain required. Fixed mode/platform cache context is valid only with identical context at peek, start and renderHead. Serialization distinguishes types and boundaries and retains exact consumed text; no Unicode normalization.

Only the listed consumed values extend that identity. K5-only browser formats, other templates, unknown stored members and scheduling-only card/deck content do not extend it: `data`, daily counters and `usn` remain excluded. Appearance, zoom and media-arrival invalidation stays with its existing owners.

One owned internal U62 input capture must feed both fingerprint and actual rendering/cache value; no new public DTO or second renderer is required. Recheck against the current head at point of use, not only on change delivery. If another render snapshot differs from the requested key's capture, return through the existing stale/fresh-head path instead of caching/serving that payload under the old key. An older in-flight result cannot replace a newer payload under its key. U62's exact-template-text parse validation remains unchanged. Identical inputs yield identical identities across unchanged peek/start and undo, without a global revision stamp.

## C7. `validUntil`

The earliest instant after now at which the head or one of its items could change without any change to the collection:

- `next_day_at`;
- for every learning entry: its `due` and its `due` − L, when after now (the instant an answer would take it into part 1, or the edge would take it in);
- for every head item and each of its learning or relearning next states with a delay of `s` seconds: `next_day_at` − `s`, when after now (from then on the delay counts as days, `sched/steps#L7`, and its label changes).

`Started`, every `Accepted`, every head returned and every head event carry it. At that instant the client asks for the head again (`head`), which recomputes states, labels and predictions, and applies P3.

## C8. Predictions

`predictedNext[rating]` is computed for each of the four ratings by applying, to a copy of the queue, the answer of the current card at the instant the head was computed: the row the rating would write (its learning delay with the learning-step fuzz of `sched/fuzz#Z7`, whose seed is known before the answer), P4 and P5, the siblings that rating would bury (P7) and a leech suspension, then P3. The prediction is the head of that copy (the current-card rule does not apply), or none.

When the answer comes at the instant the prediction was computed and nothing else changed, the head after the answer is `predictedNext[rating]` (C18, 10,000 random sessions). An answer later than that, before `validUntil`, can still meet a different order among learning cards, because the answered card's own new due moves with the answer instant.

## C9. `answer(sessionId, cardId, rating, stateToken, answeredAtMs, msTaken)`

Result: `Accepted{headDiff, counts, leech, suspended, rolledOver, validUntil}`, `Stale{freshHead}` or `NotAtHead{freshHead}`.

`answeredAtMs` is the instant of the user's input, from the same clock seam. It is the answer instant `t` for everything below (states, review-log time, P4); a value later than now fails with `InvalidArgument`, and a value earlier than the session's last answer instant is raised to it.

1. **Timing.** `sched/day-clock` supplies `(t, today, next_day_at, offset)`. C12.1 stores those values and the accepted transition. Required ordering: D8 completion before U44 application. Delayed D8 work is an intent prerequisite under C12.6; C9's return does not wait for the writer. C14 defines the new-day result and `rolledOver`.
2. **At the head** (R729): the card must be the current card, the first entry of the main list, or a card with an entry in the learning list (P4 step 1). Otherwise `NotAtHead`, nothing written (C10). Grade Now (U48) bypasses the session.
3. **Token** (R727): the session computes the card's token from the committed row, or, when an earlier answer of the same card is still uncommitted, from the row that answer will write (C11). When it differs from `stateToken`: `Stale`, nothing written (C10). It also reads the card-row digest for the op's precondition.
4. **States**: the four next states at `t` (U45, with U46's memory state when the row has none), with the session's load-balancer cache.
5. **Overlay**: apply P4/P5 and P7 to the optimistic row. Its chooser view includes each accepted review once; the real U44 commit must not append that same event twice to one cache. Preserve W8/W9 append-only, same-build one-entry undo and no-redo rules. Recompute head states, labels, tokens and predictions in this view, so `Accepted` labels equal the transition answering applies (R656).
6. **Journal and return**: enqueue the immutable C12 intent for every accepted event before returning `Accepted`, with head diff, counts, leech/suspended, rolledOver and validUntil. The registered non-writer worker meets the 1 second deadline; the UI never waits for file sync or commit.
7. **Submit** the U44 answer op at once (C11), with the session id, the session queue kind, the precondition, and the P9 record as its restorer state (`sched/answering#A13`).

`answer()` takes at most 5 ms at p95 in release, measured from call entry through the `Accepted` return, excluding the later commit. This limit also applies while a sync snapshot, a streaming `re:` search and a stats cache build run (KTD3; C18).

Antoine approved this 5 ms budget on 2026-10-21 to prioritize instant visible responses over a stricter internal timing target. R1807's input-to-visible-card requirement remains unchanged; passing this internal limit alone does not prove it. All other performance and durability limits remain unchanged.

## C10. `Stale` and `NotAtHead` (R728, R729, AE27)

- Both write nothing. The session rebuilds its queue at once (P8) and returns the fresh head. When the refused card is still presented, it stays the current card (C4) with its fresh token, states and labels, so the next answer with that token is accepted.
- The UI shows the notice "Card changed, please answer again" only when the refused card is still presented (R728); otherwise it shows the fresh head. Anki raises an error instead (P11).

## C11. Commits and `CommitFailed` (R362, R680, R1817, R2006)

- Accepted answers are submitted in acceptance order, at once; C12 provides durability within 1 s through journal or WAL coverage (`engine/op-model#O2`). Each op carries a **chain check**, run on the writer before the op writes anything: when an earlier answer of the same session failed and the session has not yet restored its queue, the op writes nothing and ends `Cancelled`.
- Two uncommitted answers of the same card (a learning card shown again before its first answer committed): the second op is submitted only after the first committed, with the card-row digest of the row the first wrote as its precondition.
- **R2006.** For each Accepted at time `a`, durable coverage is required by `a + 1 s` (C12). `WriteDelay` does not control journal admission. Application remains ordered and waits for `WriteDelayEnded` when required; acceptance continues through that interval.
- SQLite success is not undone by a lost result or a post-commit error (`engine/op-model#O4`); consult the durable assigned identity, never repeat a committed answer.
- **Failure before COMMIT** (an error, a `Stale` precondition, a panic): the session emits `CommitFailed(cardId, error)`, drops every later uncommitted answer in order (their chain checks refuse them), and restores the overlay to just before the failed answer: P9's undo for each dropped answer and the failed one, newest first, and the load-balancer entries they added. The failed card is the head again, on its question side, and a `QueueHeadChanged` follows. C12 durably resolves failure/cancellation before that resolution is final or a later conflicting commit is permitted. File failure retains recovery evidence and stops affected acceptance/commit, with an explicit durability error; it never becomes silent loss.

## C12. The pending-answer journal (KTD6, R1491, R2006)

### C12.1 Scope and intent

One device-only `meguri-pending-answers.jsonl` beside the collection, never inside it or synced. Its versioned header binds a random local incarnation to the canonical collection path and native file identity; capture that identity from the locked open file, not a later path lookup. Normal WAL writes retain it. Controlled file replacement, including maintenance that replaces the file, follows C12.7; a different native identity is not adopted silently. A registered U32 worker owns ordered append, full sync and atomic compaction. Journal **every accepted answer**, not only answers accepted during a published delay. Enqueue its immutable intent before returning Accepted; durable file coverage or a durable database commit must cover every acceptance within 1 second, including late-starting long writes and ordinary queue bursts. UI answer latency remains C9's budget; no file-sync wait occurs on that path.

An event is identified by journal incarnation and sequence. Its intent records card id, rating, effective applied `answeredAtMs` after C9 validation, elapsed time, queue kind, accepted current/chosen transition, recorded day/next_day_at/offset context, and card RowDigest or predecessor sequence. Persist the transition actually accepted, not a new choice made at reopen. A same-card predecessor resolves to its actual resulting card digest; retain these dependencies until the whole chain is terminal.

### C12.2 Semantic guards

Before ordinary delayed application and recovery, validate on the writer the card base and captured semantic inputs: home normal-deck/preset identity and home DR; effective U45 inputs under T2/T11/T12/T14; bounded U44 cap/bury/leech settings; globals read by those states (`fsrsShortTermWithStepsEnabled`, `collapseTime`); persistent day-policy inputs (`crt`, creationOffset presence/value, effective rollover); and resolved U46 memory/last-review inputs when absent from data, evaluated at the recorded instant/boundary. Use the shared T13 support check. Changed/unsupported home inputs cause explicit stale/drop reporting before answer writes.

These are known semantic projections, not whole-deck/preset/opaque-row comparisons: daily counters, unrelated mtime, unknown bytes, current queue limits/order and current chooser counts do not veto or recompute an already accepted transition. Preserve all protected bytes through the ordinary U44 field edits. C5's mtime token validation still applies before initial acceptance. FSRS now off alone does not revoke an already-admitted FSRS answer (S3); new sessions still require current U50 admission.

### C12.3 Durable assigned identity before COMMIT

Within the existing U44 transaction, after actual allocation/writes and final validation, obtain the assigned revlog id, its complete written row, resolved base digest and resulting card digest. The non-writer worker durably acknowledges a prepared receipt binding those values to the event **before COMMIT is allowed**, and before the writer takes the commit/snapshot lock. The worker needs no SQLite reader, writer operation, observer, dispatcher or commit lock. Missing/failed acknowledgement leaves COMMIT forbidden and rolls back. This is one bounded internal durability handoff, not another transaction framework.

The prepared identity is immutable: assigned id plus exact cid/usn/ease/ivl/lastIvl/factor/time/type. A prepared event never receives another id or transition. Ordinary allocation may choose the instant, largest-plus-one or the existing cap result; recovery preserves whichever was actually assigned. An intent without a prepared receipt has not yet assigned an id and may allocate normally once.

### C12.4 Commit, coverage and terminal records

States: Intent -> Prepared -> SQLite committed -> WAL covered -> durable Done. Keep intent/receipt after NORMAL COMMIT until successful real WAL coverage proves that commit durable; only then sync Done. A lost post-commit result is not rollback (O4). Live commit seqs identify WAL coverage only within that process/generation, never across restart. A failed flush advances no coverage and removes no intent. A fresh recovery flush covers its resolved database state before Done/compaction.

| Resolution | Required ordering |
|---|---|
| Cancelled / Dropped | durable terminal record precedes final cancellation, stale/failure completion and any conflicting operation |
| UI restoration | may precede terminal durability |
| terminal file failure | keep evidence; reject affected acceptance/application; return an explicit error |
| late worker acknowledgement | cannot permit COMMIT after transaction rollback |

### C12.5 Recovery before Ready

Under the collection lock and privileged open handle, validate the journal and resolve sequences before study/sync/Ready. Recheck identity and base in the writer transaction:

| Evidence | Action |
|---|---|
| Done, Cancelled or Dropped | never replay |
| prepared assigned row exists and all recorded columns match | already represented: skip answer; resolve dependent bases without overwriting later edits |
| prepared assigned row absent, matching base/context | apply the same frozen event with the same assigned id and recorded-time inputs |
| prepared id contains a different row | explicit conflict/drop report; no second id |
| intent only, matching resolved base/context | apply U44 at the recorded instant; durably prepare its actual assigned identity before COMMIT |
| no matching committed witness and stale base/context, or failed predecessor | drop/report; no answer writes; cancel dependent uncommitted events |

| Invariant | Requirement |
|---|---|
| committed evidence | exact assigned id AND complete row equality; timestamp-only predicates forbidden |
| meaning of equality | stored-record identity only, not attribution to a client |
| dependency | retain predecessor postimage; resolve predecessor before child |
| replay base | surviving SQLite prefix plus exact card digest |
| incompatible external state | report; no overwrite |
| repeated open | durable terminal event -> zero applications |

### C12.6 Recorded-time maintenance

Recovery reuses the answer transaction through a privileged opening-only mode that bypasses its ordinary **current-clock** prelude, not its checks or field-preservation rules. No global Clock rewind or current-session/head readmission is allowed.

Recognize already-committed identities first. Before applying an unresolved event, validate its captured policy/context and satisfy D8's automatic prerequisite with explicit **recorded-time** inputs in replay-local day state. This is existing automatic maintenance: no compatibility rollover repair, no creationOffset insertion, no empty lastUnburied write. Required offset/unbury writes remain separately accounted preflight writes. Repeat safely after a crash; then validate the post-maintenance card base. A stale policy/context is reported without applying a historical prelude to the changed policy.

Apply the frozen accepted transition with its recorded instant/day/boundary. Its actual sibling bury establishes that day's lastUnburied in the answer transaction. After all events resolve, discard replay-local day state and run normal **current-clock D8 once as the last open step before Ready** (lifecycle L2). It initializes the live cache quietly and performs required current unbury/offset work. Emit no historical live rollover/head events. Identity bookkeeping adds no synced data writes.

### C12.7 Commit closure and file replacement

| Operation | Required order |
|---|---|
| committed undo; owned update/deletion of unresolved revlog evidence, including sync `usn`; restoration of its base | WAL coverage -> durable Done -> operation |
| pending undo | durable Cancelled -> permit subsequent writer work; never apply the cancelled event |
| UI undo | optimistic restoration may run before the writer completes the required order |
| controlled file replacement | drain accepted events -> WAL coverage and terminal durability -> invalidate incarnation durably -> replace |
| worker callback | matching incarnation and live generation required |
| ordinary reopen | retain the bound incarnation despite a new process generation |
| unexpected file identity | retain/quarantine journal and report before Ready |

No synced UUID or configuration token is introduced. These rules prevent an old accepted event from being applied after its committed undo or to a replacement collection.

### C12.8 File integrity and completion

| Journal form | Recovery |
|---|---|
| versioned JSON; contiguous record numbers; valid per-record checksum | validate sequence and dependencies |
| final frame without newline | retain valid prefix; ignore/report tail; no durable acknowledgement or COMMIT permission came from that frame |
| complete malformed frame; checksum error; missing/interior frame; invalid dependency; unknown version | retain file; explicit corruption error; Ready forbidden |

Header creation and compaction follow U32's atomic-file and directory-sync rules. A valid stale/conflict report is separate from corruption.

Keep the chain until all events are durably terminal, then compact atomically when no declared delay is active. Suspend/exclusive flush waits for actual database and journal durability, within U32 deadline/error rules. Preserve K1/P8, all opaque/custom/preset bytes and current exact deviations. All file/barrier work counts toward existing performance budgets.

## C13. Changes made elsewhere (R456, R457, R623)

For every committed change set that is not the session's own answer, undo or redo:

- The current `sched/queue-building#B10` inputs and row precedence decide, including ownership proof, whole-collection and generation changes. **rebuild**: the session rebuilds in the background (a `SuspendParticipant`, C16), then applies the current-card rule (C4) and emits `QueueHeadChanged`. **patch**: no rebuild; the render keys of the queued cards of the changed notes and note types are computed again.
- Whatever the class, the session computes the tokens and render keys of the head items the change set touched (card, note, note type, deck, preset or config key) and reports changes in a head diff: `restate` when the token changed, `rerender` when the render key changed.
- Recompute C5/C6 fingerprints for affected note/type/home-deck/config dependencies even when their timestamps are equal. Shared-note/type edits and resolved ancestor-deck renames refresh every dependent head. Unrelated entries retain their identities/pages; memory-only changes keep C6. Complete R330 head values still refresh when their settings change with equal keys, through the existing complete-item head diff/event.
- The current card: `CurrentCardChanged{eligible}` when it is still presented and its token or render key changed (the side on screen is rendered again, R456); `{memoryOnly}` when its only change is the memory state in `data` (`s`, `d`, `dr`, `decay`, `lrt`; the change set's memory-only flag, U24): new labels and token, no new page; `{removed}` when it is no longer presented: the UI moves to the new head with "This card was changed elsewhere" (R456).
- An op the user starts from the review screen (U48's bury, suspend, flag, forget, set due date, and editing, R1812) names the session and returns a head diff in its result: the head of a fresh build at its commit instant, with the current-card rule, computed by patching or rebuilding (C18 checks it against a fresh build).
- A memory-state change of other cards keeps their render keys and changes their tokens.

## C14. A new day (R459 to R463, R600, AE13)

- On `RolledOver` (from the day clock, or from C9 step 1): the current card stays (R460). Its states, labels, predictions and token (the day part) are computed for the new day at once and sent as a head diff with `restate`. On the answer side only the buttons change (U175).
- The queue stays until the current card is answered; that answer uses the new day's states. Right after it, the session builds the queue for the new day (new limits, cards the day clock unburied, cards newly due, studied counters read as 0) and returns that head in the `Accepted` result with `rolledOver` set (R461). When the queue presents nothing (waiting, finished), the rebuild happens at once. (The conformance executor, which has no current card, follows `specs/conformance/session-ops.md` E2.)
- That rebuild clears undo and redo with cause `Rollover` (`engine/undo#M6`), as Anki drops its history when it first uses the queue of a new day: the answer of the card shown across the rollover is not undoable either, and peek reports `ClearedByRollover` (R462; P10).
- When the current card would not be gathered on the new day (the day went back, its row changed), the rebuild happens at once and the card is `removed` (R600).
- The session stays one session: same id and scope, and its answered count goes on (R463, R627).

## C15. Undo and redo in a session (R720, R722, R1811, R1812)

- Every op the session submits carries its session id; every undo step made while the session is active carries it (`engine/undo#M1`). The CloudKit hold-back reads it (`sync/policy#P8`, KTD6); `endSession` closes the scope.
- An incoming CloudKit change can end undo of a session answer. A `CloudIncoming` op (a CloudKit apply of another device's changes, `sync/cloudkit-records#K13.2`) or a `CloudRekey` op (a CloudKit re-key of a colliding local object, K14.2), published as `engine/change-stream#S1.2` says, cuts the history and redo below the newest step that touches one of its targets (`engine/undo#M9.8`, `sync/policy#P8.3`). When that step is a session answer, the answer and every older step stop being undoable, and `peek` reports the remaining history (`ClearedBySync` when the oldest step went). The queue follows C13 for the change set itself.
- `peek`, `undo` and `redo` with scope `Session(id)` are answered by the session first: its uncommitted answers come before U25's history, newest first.
- **Undo of the session's newest answer**, within 50 ms (R1811):
  - The queue goes back by P9 at once, and the card is the current card on its question side (R722). When the build identity changed since the answer, the session builds afresh at once and puts the card first (C4's current-card rule).
  - Pending answer: C12.4/C12.7 cancellation completes before its op can apply; collection writes = 0. Redo retains the rating, effective instant and elapsed time and uses C9 steps 3-7.
  - When it is committed, C12 retires its witness before U25's undo op (`engine/undo#M4`) is submitted, scope `Session(id)`; when that op fails, the session re-applies P9's redo and emits `CommitFailed` as for an answer.
- A `Global` undo while the session holds uncommitted answers waits until they commit, then targets U25's top step.
- Undo and redo of other session steps (bury, suspend, flag, forget, set due date, edit) return a head diff as C13's in-session ops do (R1812).
- Redo of an answer advances the queue by P9's redo (R720).
- The restorer of `engine/undo#M10` hands the session the P9 record of an answer undone or redone outside the session's own calls (a `Global` undo of a session answer).

## C16. Suspend, exclusive phase and end

- The session is a `SuspendParticipant` (U32): `flush_for_background()` submits every overlay answer and returns when they are committed and durable; `suspend(deadline)` stops background rebuilds (one in progress is cancelled and runs again after `resume()`).
- Entering `ExclusiveBusy` (`engine/lifecycle#L7`) drains the overlay the same way before the exclusive job's snapshot.
- `endSession(sessionId)` is called only by U175's `ReviewCoordinator`: when the summary is dismissed, when the congratulations screen shown alone is dismissed, at an exit that shows neither (KTD6, flow Q2), and at Mac quit before the close-time sync (U151). It drains accepted answers and journal durability (C12), closes the undo scope, drops the queue, then releases writer-owned admission and emits `SessionEnded`. No enable/start may pass that admission before draining finishes. Nothing else ends a session, except `startSession` (C1) and a generation change (`CollectionReplaced`), after which the session is gone.

## C17. Head diffs and events

- **Head diff**: the new head items in order (complete values) and, for each, its marks against the head the client had: `added`, `restate` (`stateToken` changed), `rerender` (`renderKey` changed), `relabel` (same token, other labels or predictions); plus the ids that left the head.
- **Events** are Session variants of the core event channel (KTD9), never coalesced away:

| Event | When |
|---|---|
| `SessionStarted{sessionId, deckId}`, `SessionEnded{sessionId}` | C2, C16; U113 consumes them inside the core |
| `QueueHeadChanged{sessionId, diff, counts, validUntil}` | a rebuild or patch changed the head (C13, C14, C11) |
| `CurrentCardChanged{sessionId, cardId, eligible \| removed \| memoryOnly}` | C13 |
| `CommitFailed{sessionId, cardId, error}` | C11, C15 |
| `WaitingUntil{sessionId, instant}` | nothing is presented and the learning list holds held entries: `instant` = the smallest held `due` − L, when P3 will present that card (R622; U186 is its UI) |
| `Finished{sessionId, state}` | nothing is presented and the learning list is empty; `state` is U41's `DeckStudyState` (`sched/counts#C7`) |
| `PresetUnsupported{sessionId, presetIds}` | a rebuild meets a home preset Meguri cannot schedule (`sched/fsrs-policy#U4`); answering a card of it fails with `UnsupportedParams` and writes nothing |

### C17.1 Terminal state in the head

Every head carries `terminalState`, a `ReviewTerminalState` value:

| Value | Required session state |
|---|---|
| `Active` | At least one head item is presented. |
| `WaitingUntil{instant}` | No item is presented, and the session retains held learning. The instant is C17's smallest held `due` minus L. |
| `Finished` | No item is presented, and the session retains no learning entry. This is a marker, without a `DeckStudyState` payload. |

Capture this value with `items`, `counts` and `validUntil` from the same session
projection, after P3. Include it in start results, answer outcomes, refreshed
heads, restores and head diffs. Accepted answers use the accepted overlay and
queue before the writer commits. Computing this value must not drain the writer
or perform a separate deck query. C9's optimistic acceptance and budgets remain.

Counts exclude held entries. C7's deadline does not identify the reason it
expires. Neither field replaces this explicit terminal state.

The client selects its review phase from the head it applies. It must not
classify an empty head from a separate committed `DeckStudyState` query.
That query may supply summary facts; those facts do not replace the head's
terminal state. `Finished` means this session has no presented or held card,
even if daily limits or buried cards leave other collection work.

Session events retain C17's meanings and ordered delivery. A terminal event
requests a head refresh; its payload must not override a newer applied head.
Use the existing session identity and operation ordering when applying results.
This contract adds no revision counter and no new persistence or sync format.

Own regressions must check both directions with a fixed clock and a held writer:

- Accept Easy on the last learning card inside learn-ahead. The head is empty
  and `Finished` before commit, while a committed deck query still sees learning.
  Release the writer. The next head remains `Finished` at the same clock instant.
- Accept Again with a 60-minute relearning step and 1,200-second learn-ahead.
  The empty head is `WaitingUntil` before commit. After commit and repeated head
  reads, its instant remains the same. At readiness, P3 presents the card and
  the head becomes `Active`.
- The bridge preserves all three values. The coordinator follows each applied
  value despite conflicting committed summary facts or an older terminal event.

For native coverage, keep a learning card beyond the session's learn-ahead edge.
The `local-review` recipe uses `fixtures/synthetic-collections#F9.2`'s zero
learn-ahead override. Its fixed-clock Check Now action retains `WaitingUntil`.
The ordinary 10-minute step with 1,200-second learn-ahead still follows P2:
the card is presented early, and Easy can exhaust the session.

## C18. Meguri tests

`core/crates/meguri-engine/tests/session.rs`, `session_faults.rs`, `session_predict.rs`, the Swift `ReviewSessionBridgeTests`, and the session benches cover what the oracle cannot see. Each is one of the plan's U43 test scenarios: the R621 prediction cases and 10,000 random sessions (C8); `WaitingUntil` at `due` − L (C17); AE27 (C10); `NotAtHead`; `memoryOnly` with the next two heads keeping token and render key (C13); `rerender` for a field edit and `restate` for a preset change; the render cache counter and no render in `answer` (C6); the load-balancer label update in the same `Accepted` (C9); AE13 at 03:59:58 (C14); AE22's session part (C15: A first after undo across a background rebuild); `flush_for_background` with 5 overlay answers (C16); U25's held-back run with [answer1, answer2, bury] in one session; undo of an answer pending behind a 3 s declared write; a `Global` undo waiting for two uncommitted answers (C15); commit failure on answer 3 of 5 (C11); kill 1 s after an accepted answer; kill during a declared write and two reopens (C12); `startSession` cancelling optimizer training (C2); the two refusals (C2); `peekFirstCard` at 150k (C3); the performance budgets (C2, C3, C9, C15: `answer()` ≤ 5 ms p95, `renderHead` ≤ 5 ms p95 for a Migaku-like card on the M3 Max, `startSession` plus `head(3)` ≤ 150 ms at 14.4k, undo to head ≤ 50 ms also during a 200 ms `SyncIncoming` commit). In addition: the P11 differences, and the in-session ops of C13 against a fresh build at the same instant.

Mandatory additional evidence:

| Contract | Cases |
|---|---|
| C2 / S3 | writer admission versus enable/end; unsupported homes on suspended/buried/future cards; invalid global Easy Days in an unused preset; admitted FSRS-off continuation |
| C5 / M2-M3 | next_day_at-only changes; resolved history changes without a card-row edit |
| C5 effective home inputs | two same-second parameter/step/retention edits with equal id/mtime; stale prior token writes zero; later unsupported list; bounded cap/bury/leech changes |
| C5 identity domains | with existing stamps fixed, unknown/runtime/unused preset metadata leaves tokens equal; changed required preset mtime still changes before-acceptance identity but alone does not reject C12's accepted intent |
| C6 / C13 | two same-second field/tag edits; selected template/CSS/type/template-name/field/type-in metadata edits; home/ancestor deck rename; correct new package with unchanged stamps |
| C6 cache binding | race older/newer snapshots and cache insertion; no payload under a different capture's key; unchanged peek/start; repeat cache hit; answer renders zero; display config/platform/mode isolation |
| C5 / C6 preservation | flag: C5 equal, C6 changed; usn-only: both equal; memory-only: C6 equal/no render; unrelated heads retain both keys; unknown/unselected render inputs with existing stamps fixed add no invalidation |
| C9 / W8-W9 | two optimistic answers of one card; each accepted cache contribution counted once across commit |
| C12.1 | declared write starts after Accepted; ordinary queued burst exceeds 1 s; every acceptance satisfies its deadline |
| C12.3-C12.5 | crash at each state boundary; taken/capped id; different row at assigned id; unrelated future revlog; same-card committed prefixes; prepare/WAL/terminal sync failures |
| C12.6 | historical day/offset replay, missing creationOffset, then current D8; changed home/global/policy inputs cause stale resolution |
| C12.7 | pending and committed undo; file replacement; late acknowledgement |
| C12.8 | incomplete final frame versus complete/interior corruption |
| conformance/normalized-dump#N11.1 | real 200-answer session API preservation proof |
| sched/fsrs-policy#T3 | session/provider obligations |

The original C18 suite, bridge assertions and performance limits above all remain mandatory.

## C19. Scenarios

All under `conformance/scenarios/sched/session/`, with goldens; the executor rules are `specs/conformance/session-ops.md`.

| Scenario | Covers |
|---|---|
| `drain/mixed-40` | R1894: a full drain of 40 cards (16 new, 9 intraday learning with one relearning, 3 interday learning, 12 reviews, in a deck and its child), Good with Again every 5th answer, the clock moving 10, 40 and 25 s between answers; 82 answers; queue order, kinds and counts after every answer, with the two cards held at the start taken in as answers move the cutoff (P2 to P6) |
| `requeue/after-next-learning` | P5 rule 1 (moved after the next learning card; the row's due unchanged), P2's fixed cutoff, the card shown again when alone |
| `requeue/front-rules` | P5: the first entry never attempted; the first entry's due at the edge |
| `requeue/main-not-empty` | P5 with a main list; P2: the learning card enters part 1 at the next answer after its due; P8: a rebuild resets the cutoff |
| `cutoff/refresh-when-empty` | P3, R622's ready instant `due` − L |
| `head/learning-anywhere` | P4 step 1: a learning card answered from part 3 and from the held part; the learning count's lower bound |
| `undo/answer-undo-redo` | P9: undo and redo of two answers, the front rule, counts, the cutoff after redo |
| `rollover/ae13` | P10, C14: an answer across 04:00 on the old queue, the new day's queue (unburied sibling, new-card limit back), the history empty (AE13's core part) |
