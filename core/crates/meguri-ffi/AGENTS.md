# meguri-ffi

- Every export returns `Result<_, CoreError>` (a panic in a non-throwing export crashes Swift) and starts with `callback::enter("<name>")`. Object constructors too.
- Call into Swift only through `callback::invoke`, never while holding a lock. No async foreign traits (KTD9).
- The exported surface is identical on every slice of one configuration: gate implementations with `cfg` inside a function, never an export, a signature or a record field. `build-core-xcframework.sh` compares the `uniffi_*` symbols and the bindings generated from each slice, and fails otherwise.
- Test-only exports go in `test_hooks.rs` (feature `test-hooks`). Never add a test-only `CoreEventKind` variant: CoreBridge switches over the enum exhaustively and must compile against AppStore bindings, which have no test hooks. Test events reuse `JobFinished` with ids from 1,000,000.
- The event dispatcher and the job registry are process-wide. Rust tests that set the event listener must tolerate other tests' events (`cargo test` runs them in one process).
- One engine per process (`runtime::engine()`, no collection until U37 opens one through `CoreHost::open`). Jobs run on its registry and bounded pool; `CancelToken` and `JobProgress` wrap its token and sink. The dispatcher is a subscription to its change stream: `seq` is the stream's, and kinds without a `CoreEventKind` variant are skipped until their unit adds one.
- Rust's objects embed LLVM bitcode newer than Xcode's: inspect libraries with `nm --no-llvm-bc`.
