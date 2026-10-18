# CoreHost composition root

Spec: engine/host
Requirements: R1658, R2008, R2020; KTD2
Anki: none (Meguri design, U38)
Scenarios: conformance/scenarios/undo/; host tests in core/crates/meguri-host/tests/

`meguri-host` constructs the engine and its providers. App, server, CLI and headless engine operations use this composition root. Raw storage conformance has a separate boundary (H4).

## H1. Configuration, ownership and lifetime

`CoreHost::open(HostConfig)` receives the collection directory, backups directory, App Group snapshot path, clock, random source and `DiskSpaceProvider`. It uses these same inputs for the engine and each provider. Callers supply service inputs; they do not construct or register engine ports.

Only this crate calls the engine's raw open entry point outside engine tests. It constructs providers before opening and runs the full engine lifecycle, including its automatic day-clock check before `Ready`. The automatic policy is `sched/day-clock` D8; it does not substitute an explicit compatibility timing query.

One host owns one open engine, its collection writer and the providers' lifetimes. It owns their shutdown even when a caller drops its collection handle. The collection has one writer; providers never open a second writer to the collection file. Their own sidecar databases may have separate writers. Provider code never waits synchronously for a new engine operation while it occupies the engine writer.

## H2. Existing derived and media providers

- Wire U26's real `DerivedHost`: open step, commit observer, generation listener and suspend participant. Its open step runs first, before any open-time operation commits. Its generation binding and reader attachments follow `storage/derived-store`; it is not a pending provider.
- Wire U31's real media store and recovery open step. Recover interrupted publications against committed collection references before exposing `Ready`. Do not treat a publication still owned by a live operation as abandoned (`media/media-journal`). Recovery failure fails the open; it never exposes a partially composed host.
- Register the providers' real suspend, connection-release and resume behavior. During suspension, quiesce their work before releasing connections. Close the media journal connection only after operations and readers that need it have drained. Deadline-aware release must return within the caller's deadline; a timeout reports failure instead of pretending suspension succeeded.
- Resume reopens provider-owned connections before admitting their dependent work. Derived invalidation and media recovery remain generation-aware. Neither provider modifies the collection schema or injects launch-time config bookkeeping.

## H3. Bindings, unavailable providers and milestone checks

The wiring catalog identifies semantic bindings, their required port roles and their owner units. Each binding has exactly one real or explicitly unavailable provider. Multiple bindings can implement the same trait; several commit observers are valid. Counting one provider per broad `PortKind` is insufficient to prove complete wiring.

| Binding | Owner | U38 state |
|---|---|---|
| Derived storage lifecycle and commit capture | U26 | Real provider |
| Media recovery and connection lifecycle | U31 | Real provider |
| Safety-backup service | U127 | Explicitly unavailable until its provider lands |
| Queue and count observer | U41 | Explicitly unavailable until its provider lands |
| Review replay and session suspension | U43 | Explicitly unavailable until its provider lands |
| Sync policy and lifecycle subscriptions | U113 | Explicitly unavailable until its provider lands |
| Stats observer | U95 | Explicitly unavailable until its provider lands |
| Widget snapshot builder | U100 | Explicitly unavailable until its provider lands |

`wiring/pending.rs` names every unavailable binding and its provider unit. An unavailable feature reports that state when invoked. Opening an otherwise usable collection can succeed while such features remain unavailable. An absent provider must never look like a completed backup, replay, snapshot or subscription.

In particular, an unavailable backup service refuses a backup-required operation before its mutation. The refusal preserves data, undo history and the change stream. It must not return success, omit a required backup or fall back to an adapter's direct write. U127 later replaces that provider with the package backup service.

Port tests reject omitted or duplicate bindings and discrepancies between the catalog and actual registrations. The milestone check resolves each pending owner through `docs/process/milestones.yaml`. It rejects an owner in the gated milestone or an earlier milestone. Thus M1R permits only later providers U95 and U100; M1 permits none. Expected pending providers do not fail the ordinary unit gate.

## H4. Raw and hosted conformance sessions

A session has one collection backend at a time:

| Backend | Owns | Permitted access |
|---|---|---|
| Empty | No collection | Initial setup or protocol `open` |
| Raw | Storage-gate `OpenedCollection` | Storage observations, raw SQL and explicit compatibility queries |
| Hosted | `CoreHost` | Engine operations, reads and deliberate raw conformance hooks through its connections |

Protocol fixture `open` and storage-level `reopen` close the current backend and return to Raw. They run the storage gate without engine open steps or a day query (`conformance/storage-ops` P1, `sched/day-clock` D14.2). Factory-only new-collection scenarios can likewise inspect raw bootstrap output. These paths do not advertise an engine operation or a production app launch.

The first engine-owned operation promotes Raw to Hosted. Close and checkpoint the raw connection first, then call `CoreHost::open` with the session's paths, clock, random source and space provider. Never keep both writers alive. Promotion runs every real provider and the full automatic engine open. Its required offset or unbury effects remain observable: do not mask dumps, alter goldens or suppress providers to hide them. A failed promotion returns an error; it does not retry the operation through Raw.

Keep Hosted until an explicit close, storage reopen, fixture replacement or handoff closes it. Reads and raw observations use the active backend; they do not promote it implicitly. Once Hosted, raw SQL runs on the engine writer between operations. A statement not beginning with `select` clears both histories and invalidates cached views as `conformance/record-ops` W1 requires. It remains an intentional conformance hook, not an alternative production mutation API.

Every engine-owned write, setup operation included, uses the hosted collection's writer and operation semantics. Direct `storage_in_operations` calls cannot bypass backups, undo, observers or subscriptions. Storage-only fixture preparation and explicit compatibility queries retain their declared raw contracts. A live handoff drains and closes its backend before copying files; the next segment owns its newly opened backend.

## H5. Undo adapter activation

U38 implements `undo`, `redo`, `undo_status`, undoable `set_config` and `remove_config` through the hosted engine (`conformance/undo-ops` V1–V3). Ordinary config writes use the same engine path with their specified history behavior. A missing config key or equal-value write preserves the documented no-op and redo rules.

Activate only scenarios whose complete set of real adapters exists. U38 must not add a stand-in answer operation to complete an undo scenario. Scenarios that need U44's answer adapter remain pending until that adapter lands. `db_exec` history resets, undo rejection mapping and step-name mapping follow the existing neutral contracts.

## H6. Failure, close and parity proof

A constructor failure unwinds every provider and connection already started. It returns no collection handle and leaves no worker that can mutate the abandoned collection. Normal close stops admission, drains admitted work, closes readers, checkpoints and closes the writer, then joins provider workers. Report cleanup errors; do not report a complete close while a provider can still write. Drop cleanup must also prevent leaked workers.

Host tests prove complete bindings, constructor cleanup, shutdown and unavailable-provider refusal. Headless parity tests run the same deck-delete operation through the testkit and the FFI host entry path. Both must await the safety backup before commit and emit the same change sets. A test-controlled backup service may record order or fail deliberately; it does not count as U127's production provider. While U127 is pending, also prove that the normal unavailable provider refuses deletion before any write.

Keep the raw fixture and explicit timing goldens passing alongside these host tests. The production lifecycle, including derived and media suspension, must have direct tests; a raw storage scenario does not prove that lifecycle.
