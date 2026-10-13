# app/Packages/MeguriCore/

- `Binaries/` and `Sources/MeguriCoreBindings/` are generated and gitignored. Build them with `tools/mg core-xcframework`; `tools/mg swift-test` builds them, then runs `swift test` and fails on any compiler warning (generated bindings included).
- The Xcode scheme pre-action (U7) runs `/bin/bash <repo>/tools/scripts/build-core-xcframework.sh --xcode` with build settings from the app target: it reads `CONFIGURATION` and `PLATFORM_NAME` and adds that platform's slice.
- Every slice is arm64 only. A generic simulator destination also builds x86_64 and fails to link: build with `ARCHS=arm64` (the app sets it).
- CoreBridge imports the bindings with `internal import`: no generated type in a public signature; map to CoreBridge DTOs. Generated names collide with CoreBridge's (`CancelToken`, `CoreEvent`, `JobOutcome`, `CoreBuildInfo`): write `MeguriCoreBindings.X` in CoreBridge and in tests.
- Every FFI call runs in a lane's `call("<name>") { ... }`, which signposts it under that name (U12 owns the name registry). Never call the bindings directly from the main actor or a plain `Task`.
- Callback implementations (events, progress) only yield into an `AsyncStream`. A synchronous call into the core from a callback fails with a Rust debug assertion.
- Generated error enum cases keep Rust's capitalisation (`.Busy(runningOp:)`); other enums are lowerCamel.
- Tests call test-only exports (Rust `test-hooks`) through `import MeguriCoreBindings`, so they compile only against Debug or Profile bindings. The event listener is process-wide: tests that replace it belong in the serialized `EventTests` suite and go through `withRawListener`, which restores `CoreEventStream`.
