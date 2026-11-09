# meguri-ffi

- Every export returns `Result<_, CoreError>` (a panic in a non-throwing export crashes Swift) and starts with `callback::enter("<name>")`. Object constructors too.
- `#[uniffi::export]` requires an unqualified type name in an `impl`; import the type first. Qualified self paths fail macro expansion.
- Call into Swift only through `callback::invoke`; no async foreign traits (KTD9). Event, progress, search and host delivery, and all other callbacks, never run while holding a lock.
- Antoine approved only synchronous `HostClock` and `DiskSpaceProvider` value reads under core locks (`E-20261021T142014Z-9c3d`, 2026-10-21). Their trusted host implementations return bounded platform values, never reenter the core, and perform no UI or async work. Keep `callback::invoke` and reentry checks.
- The exported surface is identical on every slice of one configuration: gate implementations with `cfg` inside a function, never an export, a signature or a record field. `build-core-xcframework.sh` compares the `uniffi_*` symbols and the bindings generated from each slice, and fails otherwise.
- Test-only exports go in `test_hooks.rs` (feature `test-hooks`). Never add a test-only `CoreEventKind` variant: CoreBridge switches over the enum exhaustively and must compile against AppStore bindings, which have no test hooks. Test events reuse `JobFinished` with ids from 1,000,000.
- The event dispatcher and the job registry are process-wide. Rust tests that set the event listener must tolerate other tests' events (`cargo test` runs them in one process).
- One engine per process (`runtime::engine()`, no collection until U37 opens one through `CoreHost::open`). Jobs run on its registry and bounded pool; `CancelToken` and `JobProgress` wrap its token and sink. The dispatcher is a subscription to its change stream, made in the same `runtime` init as the engine, so nothing publishes before it subscribes: reach the engine only through `runtime`. `seq` is the stream's, and kinds without a `CoreEventKind` variant are skipped until their unit adds one.
- Rust's objects embed LLVM bitcode newer than Xcode's: inspect libraries with `nm --no-llvm-bc`.
- U37 retains the open host in `runtime`; opening/closing reservations exclude other owners without holding a lock across host work. Close during open is Busy. Do not close the engine from an open job: close waits for that job. Late open cancellation reports an explicit failure while retaining the live host; cancellable rollback needs an engine contract.

- RF2 callers hold coordinated native source access through terminal inspection delivery, including cancel/drain. Result objects carry typed backup publication and candidate-active failures; `JobFinished` alone does not describe rollback. Native bridge/instrumentation registration remains a later pass.

- Manual transfer previews own backend confirmations; never reconstruct ids/revisions into authority. History entries own backend ids. `CollectionLocation.manual_transfer_directory` is a separate installation-wide private root, including initial Files/open paths; profile parents cannot select it.

- A preparation task keeps the policy admission identity assigned to that host job after its typed result is consumed. Read it from the task’s stored value without I/O; never derive ownership from history entries.

- In smoke observations, open the media service before retaining a Query reader: opening that service borrows Query again.
