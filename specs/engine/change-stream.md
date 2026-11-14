# Change sets, observers and the change stream

Spec: engine/change-stream
Requirements: R1490, R1658, R1792 (core side); supports R45, R173, R447, R1286, R1434, R1559, R2051 (S1.2, S2.4)
Anki: none (Meguri design: no Anki behavior; written by the implementer, U24 execution note)
Scenarios: none; tests in `core/crates/meguri-engine/tests/`
Status: Approved 2026-10-27 (R2066) for S1.2, S2.4, S2.5 and the CloudKit wording of S3, with `sync/policy`, `sync/cloudkit-records` and `sync/cloudkit-recovery`. Every other section is in force and unchanged.

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

### S1.1 Skipped incoming IDs (U107)

Manual Anki transfers only (U107, frozen; KTD26). CloudKit applies use S1.2 and never fill these lists.

The current mutation lists describe rows actually changed. They must not list
a skipped row as added, updated or removed. Add three default-empty typed-ID
lists to the existing change set:

| Member | IDs |
|---|---|
| `skipped_incoming_cards` | incoming card IDs skipped by their own unsent live pending grave |
| `skipped_incoming_notes` | incoming note IDs skipped by their own unsent live pending grave |
| `skipped_incoming_decks` | incoming deck IDs skipped by their own unsent live pending grave |

`sync/hold-back#H10.2` defines that decision. An absent note alone does not add
its incoming card here. Merge-policy rejections for other reasons are outside
these lists. Each list is numeric, ascending and duplicate-free. IDs may repeat
across different entity kinds. Empty is the default for all other operations.

Report the exact skipped keys of this merge, not retry history. The payload is
bounded by the distinct staged incoming card/note/deck keys: at most one ID per
such key, with no row contents or per-occurrence copies. Do not silently truncate
the list. Keep these values in local operation/event data, without automatic
external logs, analytics or sync. Include no names, note text, SQL, paths,
endpoint/account values, credentials or arbitrary diagnostic strings.

These diagnostics do not affect mutation emptiness, stamps, undo or observer
invalidation. A successful operation returns them in its `OpResult` change set,
including when actual rows are unchanged. A diagnostic-only no-op publishes no
`Changes` event under S1/O4. When real mutations publish a change set, it carries
the same lists with that operation's seq and generation. A rollback publishes
no diagnostic change set and must not report the merge as applied.

Preserve the lists through model-to-FFI conversion, operation summaries and
published events. Existing constructors initialize them empty. Any adapter
reading an older serialized change set supplies empty defaults for absent
members. This adds no collection, package, server-wire or CloudKit field.
Regenerate bindings and update all consumers together; mixed generated binary
versions gain no compatibility guarantee from this source-level addition.

Focused tests cover each entity kind, duplicate and ordering rules, a missing
note without a card grave, committed/no-op results, rollback, empty defaults and
nonempty model-to-FFI/event conversion. An exhaustive constructor is updated,
not a reason to discard diagnostics required by the caller.

### S1.2 CloudKit changes (U113, U114)

Added for CloudKit sync (KTD25). S1 does not change.

- A `CloudIncoming` or `CloudRekey` op (`engine/op-model#O1.1`) reports its writes as any op does: one change set from its journal, with `source` `CloudIncoming` or `CloudRekey`, published as one `Changes` event with the op's seq and generation (S3). It adds no member to S1.
- It lists only rows the op changed. A server version whose row equals the local row writes nothing (`storage/ids-usn-mtime` S4) and is not listed, so a replayed apply publishes nothing. An entry that waits in the inbox (`sync/cloudkit-records#K13.1`) is not listed until the op that applies it commits.
- `collection_row` is true when the op set `col.scm` (a structural generation, `sync/cloudkit-records#K11.3`) or another `col` column other than `mod`. `schema_changed` follows S1 for the note types the apply changed.
- `skipped_incoming_cards`, `skipped_incoming_notes` and `skipped_incoming_decks` (S1.1) stay empty. What the apply did not apply (waiting entries, refused records, conflict copies) is CloudKit tracking state, reported in sync status (`sync/policy#P14`), never in a change set.
- Barriers and the Sync Now capture write nothing, so they publish no `Changes` event (`engine/op-model` O4).
- Media files that CloudKit installs (`sync/cloudkit-records#K9.3`) are written by the media store outside any op. They appear as the media store's own event (S3 `Extension`, U31's media arrived), not as `media_added` of a change set. Media-name entities live in the media journal, not in collection rows (`sync/cloudkit-records#K9.4`), so they are not in a change set either.
- Consumers treat a CloudKit change set like any other source's, with one exception: where a consumer has a rule keyed on source `SyncIncoming`, a change set of source `CloudIncoming` or `CloudRekey` takes that same rule. The known cases: U42's queue rebuild row (`sched/queue-building`, R457: changes from a sync rebuild the queue), and U50's post-commit `fsrs` observation (`sched/fsrs-policy` S2, its U114 row). A `CloudRekey` changes card, note or deck ids, so a queue that holds the old ids must rebuild too. Otherwise U43's session applies its rules for another source's change (`sched/review-session`, R457), observers and the derived store run as S2 says, and undo applies `engine/undo#M9.8`.

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

### S2.1 Journal-backed payload access

ObservedCommit exposes the existing journal's immutable row payload through
a small owned access handle (`storage/journal` J1-J3), rather than arrays of
all decoded rows. Preserve every changed card's complete before/after images,
added and removed review-log rows, config values on both sides and the final
collection row already specified in S2. Note contents remain excluded.
The handle binds one seq, generation and immutable journal; iteration is
repeatable and each read returns that commit's stored image even after later
commits or a collection close. Absence, spill I/O and row-decode errors are
distinct outcomes. Access errors use a result that can carry storage errors,
not only typed-row decoding errors.

Cloning or queueing the handle retains backing ownership without reading or
copying full rows. The writer keeps an owner across undo admission/discard,
answer-cache observation and registered observer dispatch. A review worker
or derived capture that outlives the callback retains an owned handle with
its captured seq/generation. Undo retention is not a prerequisite for access.
No callback or deferred consumer pins a collection transaction, and no such
transaction or journal-row handle crosses the FFI boundary.

### S2.2 Dispatch and downstream consumers

There is still one observer invocation per nonempty committed operation,
in registration order, after history/cache/restorer handling and before its
single Changes publication. Spilling does not split dispatch into row/page
notifications or remove SyncIncoming payloads. Multiple observers may read
the same rows independently. An observer that needs an owned derived delta
captures bounded work or the exact journal owner, not a second full-row copy.

QueueProvider, answer caches, ReviewService and their queue classifiers must
accept on-demand access. They must not collect or clone every full card into
a vector when source, generation, ownership or compact ChangeSet facts suffice
to classify the change. Keep queue invalidation, current-card updates and
flag-only handling unchanged. ReviewService's writer
callback still queues work without taking the service state mutex or waiting
for its worker. Its deferred work retains the same generation/seq guards and
projects only from complete successful reads.

An unreadable payload fails the affected observer under S2: log the failure,
continue remaining observers exactly once, and publish the committed
ChangeSet. It cannot become an absent card or a successful partial refresh.
Internal cache or deferred projection failures invalidate the affected
derived state through the existing rebuild/error path. They cannot mark an
incomplete projection ready or emit it as a successful head. J3 secures the
ChangeSet facts before COMMIT, so all changed keys and real note IDs remain
available even if a later observer read fails.
No new client event or wire/FFI field is required for this in-core capability.

### S2.3 Payload coverage and failure tests

Verify independent repeated reads by multiple observers, queued ownership
after undo discard and later writes, exact same-commit images, and all
existing dispatch/publication ordering rules. Inject image read/decode
failures and prove the absent/error distinction, observer isolation, cache
invalidation and deferred projection failure behavior. Preserve no-op and
whole-collection observer behavior. Cover revlog and config access too: a
card-only test cannot support a general observer-payload memory claim.
Run the allocator-backed 150k payload proof in `storage/journal` J4 through
real consumers; existing dispatch, held-run and large-sync tests alone do
not establish that memory bound.

### S2.4 The CloudKit tracking observer (U113, U114)

Added for CloudKit (`sync/cloudkit-records#K11.1`, `#K11.2`, `#K24`). S2 to S2.3 do not change for other observers.

- The tracking observer is an in-core `ChangeObserver`. U114's wiring registers it at every collection open at which the CloudKit tracking store exists or iCloud sync is enabled on the device, before the writer admits ops. It then runs for every op with a non-empty change set, whatever its source. Ops committed while it was not registered are found by the reconcile scan (`sync/cloudkit-records#K11.4`) or by the first connection (`sync/cloudkit-recovery#V13`).
- Beyond S2's payload it reads, through the same journal-backed handle (S2.1), what the journal already keeps for every journal row of every table: the before-image (the base of `sync/cloudkit-records#K11.2`), and the after-image, which for notes is the journal's digest of the new row (`engine/undo` M1). That note digest is the one K11.1 compares only when it equals K11's row digest for the note; otherwise the observer reads the live note row through the writer's connection, which equals the after-image because observers run before the next op (S2). The journal keeps nothing new for this (`storage/journal`, U23, unchanged). The tables are notes, decks, presets (`deck_config`), note types with their `fields` and `templates` rows, and tags, as well as cards, review-log rows and config values. The payload of other observers stays as S2 says.
- It learns of its own failure: when its run returns an error or panics, the dispatcher still lets the commit stand and runs the other observers (S2), and it then calls the observer's failure hook once, on the writer, with the op's seq. The tracking observer sets its scan-required flag there (`sync/cloudkit-records#K11.1`).
- For a `CloudIncoming` op it records the op's seq in the apply intent and marks no key of the intent dirty, `entry` or `side_effect` (`sync/cloudkit-records#K11.1`, `#K13.2`): the settle after the WAL flush makes the `side_effect` keys pending as local changes. The observer does not run for an empty change set, so the core also records the seq from the op's result (`engine/op-model#O1.1`); both writes are idempotent.
- For a `CloudRekey` op it records the op's seq in the re-key intent, and marks the reference rows the op rewrote dirty as local changes (`engine/op-model#O1.1`, `sync/cloudkit-records#K14.2`). The re-keyed entity itself moves to its new key when the intent settles (K14.2), not here.
- It reads the op's structure report (S2.5) and stores from it the op's structural generation, if any (`sync/cloudkit-records#K19.2`). It never infers field or template maps from rows.
- Tests: a note edit, a deck rename, a preset change, a note type field rename and a tag registration each give the observer the before-image and the after-image or note digest; an observer that panics gets one failure call with the seq, and the other observers and the publication are unchanged; a `CloudIncoming` op marks nothing dirty, and a `CloudRekey` of a note marks its cards' rewritten rows dirty.

### S2.5 Structure reports (U113, U114; `sync/cloudkit-records#K19.2`)

Added 2026-10-27 for CloudKit structural generations (R2053). S1 to S2.4 do not change. Decided 2026-10-27: the op that changes a structure states its maps, because no observer can infer them: a field rename and a reorder leave the same `fields` keys (S1), and moved notes do not say which old field fills which new one. A guessed map would put field contents in the wrong fields on every receiver.

- **The report.** An op that changes a note type's fields or templates, removes a note type or moves notes to another note type fills a structure report through its op context before it returns:
  - `reasons`: the `sync/cloudkit-records#K19.1` reason codes that apply to the op, ascending;
  - `layouts`: per note type that exists before and after the op and whose `fields` or `templates` rows the op added, removed or renamed (a different name at an `ord`, so a reorder counts), or whose sort field it changed: the note type id, `field_map` and `template_map`, with K19.2's meaning (entry i is the old `ord` that new `ord` i comes from, or -1 for none). A save that only renames reports identity maps and no reason code;
  - `removed`: the ids of the note types the op removed;
  - `type_changes`: for Change Note Type and an import's merge (`package/notetype-merge#J9`), one entry per (old note type, new note type) pair whose notes moved: the old id, the new id, and the field and template maps from the old note type's `ord`s to the new one's. So a note type whose notes go to two note types gets two entries (decided 2026-10-27). When either note type of a pair is a cloze note type, the change has no template map (`notetypes/change-notetype#M1`) and cards keep their ordinals (`notetypes/change-notetype#M9.2`, `#M9.3`): the entry's template map is K19.2's prefix map, empty when the old note type is a cloze note type and the identity list of the old note type's template count when it is standard (decided 2026-10-27, K19.2 "No template map").
  `struct_gen` values are not in the report; tracking assigns them (K19.2).
- **Ops that report.** Every op that K19.1 lists fills the report with its reason codes, also when its maps are empty: a preset deletion reports 7 and no layout; every `.apkg` import reports 8 `apkg_apply`, whatever its size. Decided 2026-10-27: an import also reports 3 `sort_field_changed`, with that note type's identity layout, when its update without a merge rewrote a note type's sort field (`package/apkg-import#I7`); an identity layout, with no reason of its own, for each updated note type whose field or template names changed; and 9 `notetypes_merged` only when the merge has a structural effect (`package/notetype-merge#J9`), with J9's layouts, `removed` and `type_changes`. A merge without such an effect reports the reason 8 alone (with identity layouts for its renames). Check Database reports 10 only for the repairs that `notetypes/schema-changes#G2` lists as schema changes. A note type save that changes nothing structural (`notetypes/schema-changes#G3`) and is not a rename reports nothing.
- **Scope.** The report travels with the observed commit (S2), beside the change set. It is not a change-set member (S1.2 adds none), is not in a `Changes` event and does not cross the FFI. A failed step of a composite op rolls its part of the report back with its savepoint, as `engine/op-model` O3 does for the op's other notes; a rollback of the op discards it.
- **One op, at most one generation.** When `reasons` is not empty, the tracking observer stores one generation for the op, with state `local`: its reasons, its `maps` from `layouts`, its `type_changes`, and its members the entities the op changed (K19.2). An `.apkg` import is one op in one transaction (`engine/op-model` O3), so it is one generation whatever its size. An op whose `reasons` is empty makes no generation; its identity maps only show that a rename was not a reorder.
- **Undo and redo.** The undo manager keeps a step's report with the step, and the undo and redo ops report from it (`engine/undo#M9.11`).
- **Native recovery (Approved 2026-11-14, R2066; review: docs/process/review-log/U113-native-layout-policy-contract-20261114.md).** K13.4 retains checked live identity-layout authority through observer failure until current-open WAL coverage permits normal durable admission. Failed savepoints/rollback keep their existing report behavior; successful COMMIT/post-commit error is never rollback or replay. Exact admitted/incoming authority remains automatic; unproved ordinal meaning uses V13.6. This adds no durable native preparation or Abort.
- **The guard.** At the commit of every op whose source is not `CloudIncoming` or `CloudRekey`, the executor checks the journal: every note type that exists before and after the transaction and whose `fields` or `templates` rows were added, removed or renamed has a `layouts` entry; every note type removed is in `removed`; every note whose `mid` changed has a `type_changes` entry for its old and new note types. Decided 2026-10-27: a note whose old `mid` named no note type at the start of the transaction (Check Database's recovery of a missing note type, `maintenance/check-database-notes#N4`) needs no `type_changes` entry, since no layout exists to map from; the recovered note type is reported as an added note type under reason 10: it has no `layouts` entry (it did not exist before the transaction), and it and the moved notes are members of the op's generation like every other entity the op changed. Otherwise the transaction rolls back and the op fails with `StructureNotReported { notetype_id }`, an internal error (a bug in the op): nothing is written, as `notetypes/schema-changes#G6` does for `scm`. The guard runs whether or not iCloud sync is enabled, so a missing report fails in every build's tests. A sort-field change, a preset deletion and the import reasons are not visible to the guard; the tests below cover them. A `CloudIncoming` op applies a generation that another device reported (K19.5) and a `CloudRekey` op changes ids, not layouts; neither reports.
- Tests (`core/crates/meguri-engine/tests/op_catalog.rs`): each op of K19.1 reports its reasons and maps: a field add, removal and reorder; a template add, removal and reorder; a sort-field change; a rename alone (identity maps, no reason); a note type deletion; Restore to Default that changes structure; Change Note Type (its `type_changes`), and one from a cloze note type (an empty template map) and one from a standard note type to a cloze one (the identity list of the standard one's templates); a preset deletion (7, no layout); an `.apkg` import without a merge (8), with an update without a merge that rewrote a sort field (8, 3, the note type's identity layout), with a merge that has a structural effect (8, 9), and with a merge without one (8 alone); an import whose merge moves one note type's notes to two note types (two `type_changes` entries); a Check Database repair of G2's list (10), and its recovery of a missing note type (10, no `type_changes` entry, and the guard passes). An op that renames a field without a report fails with `StructureNotReported` and writes nothing. A composite op whose structural step fails reports nothing for that step. The undo of a field reorder reports the inverted map, and its redo the original map (`engine/undo#M9.11`).

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
  | `RolledOver{old_day, new_day}` | the day clock saw the scheduler day change, either way, and its rollover op committed (`sched/day-clock` D8, D9) |
  | `TimeZoneChanged{zone_id}` | a day clock check found an IANA zone id other than the one it last saw, whether or not the day or the offset changed (`sched/day-clock` D8) |
  | `NextDayAtChanged{next_day_at}` | the next day boundary a day clock check found differs from the one it last reported, in epoch seconds (`sched/day-clock` D8) |
  | `Extension(payload)` | a crate above the engine publishes its own event kind (U95 stats, U106 sync status (manual Anki transfers, frozen), the CloudKit sync status (U113, `sync/policy#P4` `StatusChanged`), U31 media arrived, U26 derived readiness and `DerivedApplied` (`storage/derived-store` D8), U100 widget snapshot): the engine carries it without reading it, and U6's dispatcher maps it to its `CoreEvent` variant |

- The day clock's three kinds follow the commit of the `Rollover` op whose check found them, in the order of the table, after that op's `Changes` event; their `cause_seq` is that op's seq when it published a change set, otherwise none. The first check after an open or a generation change publishes none of them (`sched/day-clock` D8).
- Ordering with observers: an op's seq is reserved at its commit and the `Changes` event is filled in once the observers ran. Events are delivered strictly in seq order: an event published by another thread with a higher seq waits until every lower seq has been filled or given up. A reservation is given up when the op published nothing (an empty change set) or its thread failed before publishing; the gap is skipped.
- Subscribers: each subscription gets every event published after it subscribed, each once, in seq order, in its own unbounded queue. It reads them blocking (with or without a timeout) or as an async receiver (crates above the engine that run their own tasks, such as meguri-servers, never on the writer thread). Dropping the subscription ends it; dropping the stream ends every subscription once its queue is read. A waiting receiver never wakes on a timer (R1854).
- Delivery to Swift: U6's dispatcher thread holds one subscription and turns each envelope into a `CoreEvent`, in seq order, never from the writer thread and never while holding a lock (KTD9). The dispatcher subscribes when the process's engine is created, before any job, op or export can publish, so events published before Swift sets its listener wait in that subscription. `Changes` carries the change set with plain ids, `CollectionReplaced` the new generation, `JobFinished` the job and its outcome; only the engine publishes `Changes` and `CollectionReplaced`. Kinds without a `CoreEvent` variant yet are skipped (U37 and the area units add variants).
