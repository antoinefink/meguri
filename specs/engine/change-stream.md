# Change sets, observers and the change stream

Spec: engine/change-stream
Requirements: R1490, R1658, R1792 (core side); supports R45, R173, R447, R1286, R1434, R1559
Anki: none (Meguri design: no Anki behavior; written by the implementer, U24 execution note)
Scenarios: none; tests in `core/crates/meguri-engine/tests/`

What a committed op reports (the change set), who sees it inside the core before anyone else (in-core observers), and the one ordered queue that carries it and every other core→Swift event (the change stream). Ops are `engine/op-model`.

## S1. The change set

A value type (`meguri_model::changes`, KTD2), derived from the op's journal (U23) after commit. Each journal entry is one row the op changed, with its image before the op (or absent) and after it (or absent). An entry absent both before and after (a row the op created and deleted) is left out. The change kind of an entry: `Added` (absent before), `Removed` (absent after), `Updated` (both).

| Member | From | Each item |
|---|---|---|
| `source` | the op | |
| `cards` | `cards` entries | card id, note id (`nid` after the op, or before it for a removed card), kind, `memory_state_only` |
| `notes` | `notes` entries | note id, kind |
| `notetypes` | `notetypes`, `fields` and `templates` entries | note type id, kind, `schema_changed`. A note type whose `fields` or `templates` rows changed but whose own row did not is listed `Updated` |
| `decks` | `decks` | deck id, kind |
| `deck_configs` | `deck_config` | preset id, kind |
| `config_keys` | `config` | key, kind |
| `tags` | `tags` | tag as stored after the op (before, for a removed tag), kind |
| `revlog_added`, `revlog_removed` | `revlog` | review-log ids |
| `media_added`, `media_removed` | what the op reports through its context (U31 stages media inside the op) | file names |
| `collection_row` | `col` | true when a `col` column other than `mod` changed (`scm`, `usn`, `ls`, ...); the `mod` stamp every committing op sets does not count |

- Every list is sorted by its key and holds each key once. `graves` entries are not listed.
- `memory_state_only` is true for an `Updated` card when every changed column is `mod`, `usn` or `data`, and between the `data` read before and after (`model/card-data-json`) only the FSRS values `s`, `d`, `dr`, `decay` and `lrt` differ. Every other card change is scheduling-relevant. Both kinds carry the note id, so consumers need no card→note lookup.
- `schema_changed` is true when the note type was added or removed, or a `fields` or `templates` row of it was added, removed or renamed (ordinals are part of the key, so a reorder shows as renames). A change of only template text, CSS or field options is not a schema change.
- A change set is empty when it lists nothing. An empty change set is never published (`engine/op-model` O4).

## S2. In-core observers

- Implementations of the `ChangeObserver` port (registered by U38: U26, U41, U95, U100) run on the writer thread, after the commit and before the change set is published, in registration order.
- An observer receives: the op's seq and generation, the change set, and the row payloads:
  - per changed card, its row before and after (absent when added or removed), read as a typed card on demand;
  - the review-log rows added (their stored rows) and removed (their before-images);
  - per changed config key, its stored value before and after;
  - the `col` row after the op, when the op changed it (every op that moved `col.mod`): the derived store's stamp (`storage/derived-store` D3, D6).
  Note contents are not in the payload (the journal keeps a digest of a note's new row); an observer that needs them reads them after the commit.
- Observers never call Swift and never submit ops and wait for them (the writer is busy running them). They keep their work short: the next op waits for them.
- An observer that fails or panics does not undo the commit: the commit stands, the failure is logged, the remaining observers still run, and the change set is still published. Data an observer derives is rebuildable (U26).
- Observers run only for ops whose change set is not empty.

## S3. The change stream

- One ordered queue per engine carries every core→Swift event except progress (`engine/jobs` J5), host events (U135) and search results (U85), which have their own channels (KTD5, KTD9).
- An event is an envelope `{seq, generation, cause_seq, kind}`. `seq` comes from the engine's one counter, shared with op seqs (`engine/op-model` O4), so it is strictly increasing for the life of the engine. `generation` is the collection generation the event belongs to, 0 when none is open. `cause_seq` is set on events derived from a change set: the seq of that change set's op.
- Kinds the engine publishes:

  | Kind | When |
  |---|---|
  | `Changes(change set)` | an op committed a non-empty change set; its seq is the op's |
  | `CollectionReplaced{generation}` | the generation changed (`engine/lifecycle` L8); exactly once per change |
  | `Lifecycle(state)` | the lifecycle state changed |
  | `Available` | the collection became available again (`engine/lifecycle` L6) |
  | `WriteDelay{op, declared}`, `WriteDelayEnded{op}` | a declared long write or a maintenance task started or ended (`engine/op-model` O8) |
  | `JobFinished{id, outcome}` | a job ended (`engine/jobs` J1) |
  | `Extension(payload)` | a crate above the engine publishes its own event kind (U95 stats, U106 sync status, U31 media arrived, U26 derived readiness, U100 widget snapshot): the engine carries it without reading it, and U6's dispatcher maps it to its `CoreEvent` variant |

- Ordering with observers: an op's seq is reserved at its commit and the `Changes` event is filled in once the observers ran. Events are delivered strictly in seq order: an event published by another thread with a higher seq waits until every lower seq has been filled or given up. A reservation is given up when the op published nothing (an empty change set) or its thread failed before publishing; the gap is skipped.
- Subscribers: each subscription gets every event published after it subscribed, each once, in seq order, in its own unbounded queue. It reads them blocking (with or without a timeout) or as an async receiver (crates above the engine that run their own tasks, such as meguri-servers, never on the writer thread). Dropping the subscription ends it; dropping the stream ends every subscription once its queue is read. A waiting receiver never wakes on a timer (R1854).
- Delivery to Swift: U6's dispatcher thread holds one subscription and turns each envelope into a `CoreEvent`, in seq order, never from the writer thread and never while holding a lock (KTD9). The dispatcher subscribes when the process's engine is created, before any job, op or export can publish, so events published before Swift sets its listener wait in that subscription. Kinds without a `CoreEvent` variant yet are skipped (U37 and the area units add variants).
