# Session scenarios: the held queue across answers, undo and redo

Spec: conformance/session-ops
Requirements: R1883, R1894, R621, R622, R722
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/session/

How the session scenarios observe a study queue that is held across answers (`specs/sched/review-session.md` part P), and how Meguri's executor mirrors Anki's held queue. They use only existing ops and kinds: `queue` (`queue-ops.md` Q2), `answer`, `undo`, `redo`, `advance`, `db_exec`, `set_current_deck`, `timing`, `card` and `undo_status`. Nothing here changes the oracle driver.

## E1. What the scenarios observe

A `queue` read while a queue is held reports that queue as `sched/review-session#P2` presents it, after the rule of `#P3` (when every count is 0 the read first moves the cutoff to now). Reads after `answer`, `undo`, `redo` and `advance` therefore show the queue as the answers changed it, not a fresh build.

## E2. Holding a queue

- **Oracle.** Anki keeps the queue it built at a `queue` read and updates it in place on every `answer` of a card it holds, and on the undo and redo of such an answer (`answer-ops.md` H2, H3). Its `queue` read asks for every presented card (a fetch limit of at least the card count), so it also applies `#P3`.
- **Meguri.** The executor holds a compatibility presentation queue built under part P, together with U44's load-balancer cache. This is not production `startSession` admission (C2): a read writes nothing, fires no hook, reserves no session admission and selects no deck. Queue-only cases with FSRS off remain valid. No card is marked as shown.
  - `queue` reports P2/P3 presentation, kinds and counts;
  - `answer` applies ordinary U44 checks at the step instant, then advances the held queue under P4-P7; learning-anywhere is allowed, other non-head answers fail;
  - owned answer/undo/redo commits update presentation under P9 and cache under `sched/load-balancer#W8`/W9 exactly once; `next_states` uses that cache;
  - the compatibility queue does not prove production tokens, admission, overlay, journal or C5 API behavior. C18 tests those separately.
- **Dropping.** H3 drops this presentation state and its cache; the next read builds again. Production sessions instead apply B10's observer and ownership classification as changes commit (C13).
- **A new day.** The next queue read drops the old build/history under P10. One answer before that read can advance the old presentation with the new boundary; read immediately afterward, before another clock change. E3 rule4 retains this bounded observation schedule. Production shown-card behavior remains C14.

## E3. Writing session scenarios

1. **Hold the queue** with a `queue` read after the last raw write (a `db_exec` that is not a `select` drops it, `record-ops.md` W1), and keep the load balancer off unless the scenario is about it (`answer-ops.md` H4).
2. **Answer at the head**: the card the queue presents first, or a card the queue holds in its learning list (`#P4` step 1). Anki fails any other answer with an error that stops the run (`anki_error`); Meguri's `NotAtHead` is checked by Meguri's tests.
3. **After a drop**, read `queue` before the clock moves.
4. **A new day**: only with the load balancer off; observe `timing` right after crossing (`sched/day-clock#D14.2`), then answer at most one card on the old queue and read `queue` right after it; observe `undo_status` only after that read.
5. **Distinct learning keys**: never let two learning entries share a key (`#P5` rule 3): no two learning cards with the same `due` and the same `reps`-is-zero state.
6. **Undo within one build**: no drop between an answer and its undo (Meguri shows the card first after a rebuild by design, `#P9`, `#P11`).
7. **The undo status**: `set_current_deck` records an Anki-only step (`queue-ops.md` Q1), so a scenario that observes `undo_status` either studies the Default deck without it, or observes only after a new day emptied the history (rule 4). Its state `db_exec` comes before the first `queue` read (`undo-ops.md` V5).
8. **Long drains** advance the clock by amounts that do not repeat with the rating policy's period, so that no card is answered Again forever.

## E4. Evidence and activation

Historical receipts identify the prepared artifacts, before the privacy repair:

| Receipt | Result |
|---|---|
| record `20261017T202541Z-96560` | PASS 8/8 |
| record `20261017T202613Z-98822` | PASS 2/2 |
| Linux compare `20261018T054843Z-5955` | PASS 8/8 |
| native compare `20261018T055309Z-13969` | PASS 8/8 |
| whole corpus `20261018T055009Z-10577` | 431 PASS / 41 FAIL |

Current evidence:

| command | args | result.json | commit | dirty | status | total/passed/failed/skipped |
|---|---|---|---|---|---|---|
| `tools/mg oracle` | `record sched/session/rollover/ae13` | `build/mg/oracle/20261021T035447Z-19239/result.json` | `d4ee2d87` | false | PASS | 1/1/0/0 |
| `tools/mg oracle` | `compare sched/session/` | `build/mg/oracle/20261021T035529Z-20184/result.json` | `d4216c3e` | false | PASS | 8/8/0/0 |
| `tools/mg oracle` | `compare --platform apple sched/session/` | `build/mg/oracle/20261021T035540Z-20857/result.json` | `d4216c3e` | false | PASS | 8/8/0/0 |
| `tools/mg goldens-check` | `--base f53b01f634026681eeaf0addc4682fa81a4ba589` | `build/mg/goldens-check/20261021T035555Z-21314/result.json` | `d4216c3e` | false | PASS | 627/627/0/0 |

| schema_files | schema_errors |
|---|---|
| 1372 | 0 |

| Pending scope | Status | Required evidence |
|---|---|---|
| Product activation | PENDING | D8 / D10; both product lanes |
| C5 | PENDING | C18 |

## E5. Deck-flow scenarios (list)

The scenarios under `sched/session/deck-flow/` study a deck through several answers on one held queue (E2), following the rules of E3.

| Scenario | Covers |
|---|---|
| `sched/session/deck-flow/parent-then-children` (held) | `sched/orders#O2`: new cards gathered deck by deck across a parent and its nested subdecks in unicase name order; counts after each answer |
| `sched/session/deck-flow/child-review-limit` (held) | `sched/limits#L3`, `sched/limits#L4` and `sched/answering#A6`: a review answered in a subdeck lowers the parent's remaining reviews in the held queue, in the deck list and in a later study of the parent |
| `sched/session/deck-flow/new-within-review-limit` (held) | `sched/limits#L2`, `sched/limits#L4`: new cards admitted within the review limit; counts after each answer until the learning card graduates and the queue is empty |
