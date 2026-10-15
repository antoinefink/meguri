# meguri-engine

- Every write to `collection.anki2` is an op on the executor (`Collection::submit`/`run`; `specs/engine/op-model.md`). Only the writer thread holds the writer connection. Open collections only through `Engine::open_raw` from meguri-host (this crate's tests and benches may name it).
- Op closures and `ChangeObserver`s run on the writer thread: never submit an op and wait for it there (deadlock), never call Swift, keep them short. A write phase past ~200 ms must be a catalog op (`executor/catalog.rs` and spec O9 together).
- A write phase never writes a snapshot value over a row that may have changed: re-derive inside the op or use a `Precondition` (O7). Never mtime or `usn`: `Local` stamping has one-second resolution.
- Op seqs and event seqs are one counter. An op reserves its seq at commit and fills it after the observers; an unfilled `Reservation` holds back every later event, so let it drop (never `mem::forget`).
- Lock order: lifecycle state, then executor queue, then commit lock or stream. The writer thread never takes the lifecycle lock.
- `close` and a replace wait for every reader lease: an exclusive job drops its reader (and snapshot) before `Exclusive::replace`. That wait is what keeps the generation fixed while a lease is held, so check a handle's generation inside `Readers::lease`, never before taking the lease.
- Every thread through `ThreadRegistry::spawn` (named, QoS, idle list for U32). No timed wake-ups while idle (R1854): wait on condvars without timeouts unless a caller asked for one.
- `monotonic.rs` is the crate's only system-clock read; collection time comes from the `OpenConfig` clock.
- Tests open copies of `conformance/fixtures/storage/` (`tests/common`) and never name `CollectionBuilder` (layering-check). The 150k reader test uses testkit's `bench::fixtures::profile_collection`, cached in `build/fixtures` (`tools/mg fixtures 150k` makes it).
