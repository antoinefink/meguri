# Rust core ↔ SwiftUI bridge and build system (Meguri)

Researched 2026-10-12 on this Mac. Labels: **[measured]** means a prototype was built here. **[doc]** means official docs, changelog or crates.io. **[2°]** means a forum or vendor source.

The prototype is in `scratchpad/proto` of this session. It contains a UniFFI 0.32.2 crate that uses fsrs 6.6.2, an XCFramework, and a SwiftPM package with Swift Testing.

## 0. Local toolchain facts

- Xcode 27.0 (27A266a), SDKs macOS 27.0 and iOS 27.0, Apple Swift 6.4 (swiftlang-6.4.0.34.1).
- Rust 1.89.0 has only `aarch64-apple-darwin` installed. Current stable is **1.99.0** (2026-10-01).
- The iOS 27.0 simulator runtime is installed. Its default devices are iPhone 18 Pro/Pro Max, 17, 17e, Air, and iPads (M5 Pro, M4 Air, A16, mini).
  - **There is no "iPhone 17 Pro" device by default.** The device type exists: create it with `xcrun simctl create "iPhone 17 Pro" com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro com.apple.CoreSimulator.SimRuntime.iOS-27-0`.
  - **No simulator device type exists for a foldable iPhone in Xcode 27.0.** Foldable layouts can only be approximated with size classes or iPad mini, or tested on hardware.
- `xcrun mcpbridge`, `mcp-server` and `agent` binaries are present in Xcode's `usr/bin`.

## 1. Bridge choice: UniFFI 0.32.2 with proc-macros (recommended)

### Versions [doc]

- `uniffi` / `uniffi_bindgen` **0.32.2** (2026-09-22). 0.32.0 came out 2026-06-30. 0.32.1 and 0.32.2 are Kotlin-only fixes.
- MSRV is **1.87** (since 0.31.0; CI uses 1.90).
- Cargo features include `tokio`, `cli`, `ffi-trace`.

### Proc-macro vs UDL

- **Use proc-macros only:** `setup_scaffolding!()`, `#[derive(uniffi::Record/Enum/Object/Error)]`, `#[uniffi::export]`.
- UDL adds a second source of truth and nothing that Meguri needs.
- Since 0.31, `uniffi-bindgen` always runs in "library mode" from the compiled `.a`. Multi-crate workspaces work: bindings cover every UniFFI crate linked into the one staticlib.

### Object handles [doc]

- Objects are `Arc<T>` with `T: Send + Sync`. Since 0.30 they cross the FFI as `u64` handles. The Swift class `deinit` frees the handle.
- Generated Swift classes and protocols are `Sendable`.
- A `protocol XProtocol` is also generated, which is useful for mocking in SwiftUI previews and tests.
- Since 0.31, methods on records and enums are supported.

### Errors [doc + measured]

- A `#[derive(uniffi::Error)]` enum becomes a Swift `enum: Error`, with `LocalizedError` unless you turn it off. Functions become `throws`.
- **Rust panics are caught** (`catch_unwind`) and become an internal Swift error. In a non-throwing function, that error is a fatal crash.
  - Therefore: **every exported function returns `Result`**, and **`panic = "unwind"` must stay** in all profiles.
- Verified: `CoreError.Db(msg:)` was caught as a typed Swift error.

### Callbacks / foreign traits

- Use `#[uniffi::export(foreign)]` for Swift-implemented traits and `#[uniffi::export(rust, foreign)]` for traits implemented on both sides. `with_foreign` is deprecated.
- The Swift implementation must be `Sendable`.
- **Sync foreign traits compile clean in Swift 6 mode** [measured]. A 0.31.2 fix removed the strict-concurrency warning on vtables.

### Async [measured]

- An exported `async fn` (Rust→Swift) **compiled clean in Swift 6 language mode** with 0.32.2 and Swift 6.4.
- **Async foreign traits (Swift-implemented `async` methods) fail with a hard error in Swift 6 mode:** `#SendingClosureRisksDataRace` in the generated `uniffiTraitInterfaceCallAsync…` helpers.
  - This is issue #2929. Fix PR #2943 is merged on `main` but **not in any release**: the v0.32.2 tag diverged from it.
  - Issue #2458 (async not Sendable) is still open.
- **Swift `Task.cancel()` does not cancel the Rust future** (PR #2887 is open). The Rust future runs to completion.
  - Long operations need explicit cancellation: a Rust `CancelToken` object with `cancel()`, checked inside sync, import and optimize loops.

### Swift 6 strict concurrency mitigation (verified)

- Put the generated `.swift` file in **its own SwiftPM target compiled with `.swiftLanguageMode(.v5)`**. Hand-written wrapper and UI targets stay `.v6`.
- With that split, even the async-foreign-trait case builds with zero diagnostics.
- Language mode is per module, so this costs nothing at runtime.

### Per-call overhead [measured]

M3 Max, macOS, Rust fat-LTO release:

| Call | Swift debug | Swift release |
|---|---|---|
| no-op method on an object | 0.14 µs | **0.009 µs** |
| `next_card()` returning a record with 2 strings + 2 ints | 2.1 µs | **1.4 µs** |
| `answer()` incl. `FSRS::new` + `next_states` (fsrs 6.6.2) | 0.64 µs | **0.26 µs** |
| `&[u8]` argument, 50 MB `Data` (zero-copy since 0.32) | 14 µs | **7 µs** |

- Against the ~1 ms answer budget, the FFI and FSRS cost is negligible: about 0.1–0.3% of the budget.
- The budget is set by SQLite I/O and by Swift-side work.
- A competitor's README (BoltFFI, [2°]) reports a ~1.4 µs UniFFI no-op. That figure matches the debug-ish numbers above.

### Large data

- Strings, records, `Vec` and return values are **serialized and copied** through `RustBuffer`. This costs microseconds for card HTML of 10–100 KB.
- Zero-copy applies **only to `&[u8]` arguments of sync functions** (Swift `Data`). It does not apply to returns or to async functions.
- Media should never cross the FFI. Pass file paths, and let `WKURLSchemeHandler` read the files from disk.
- For bulk data (browser pages of 200 rows, stats), return one record holding a `Vec<Row>`, or a protobuf/bincode `Vec<u8>` blob. Never make one call per row.

### Alternatives (brief)

- **swift-bridge 0.1.59** (2026-01-06; about 2.1M downloads vs UniFFI's 14M):
  - Pros: Swift-native types and slightly lower overhead.
  - Cons: pre-1.0, slower release cadence, a smaller maintainer base, and more hand-written bridge code.
  - Not worth it, because UniFFI's overhead is already in the µs range.
- **Hand-written C ABI + cbindgen:**
  - Pro: the lowest overhead.
  - Con: every string, error, ownership and lifetime rule is written by hand. That is a large unsafe surface where autonomous agents would introduce undefined behaviour.
  - Reject.
- **BoltFFI 0.31** (created 2026-02): claims sub-ns calls, but it is 8 months old. Do not adopt it. Revisit only if profiling ever shows the FFI to be hot.

**Recommendation: UniFFI `=0.32.2`, pinned exactly.** The bindgen binary and the runtime crate must match exactly, because checksums and the contract version are compared at load time.

### API shape that avoids the rough edges

- Export **synchronous** Rust functions and methods. Call them from Swift off the main actor: a `CoreService` actor or `@concurrent` functions in the wrapper package.
- Rust owns its own threads and tokio runtime for sync, the local HTTP servers, and background optimization.
- Report progress through **sync** foreign-trait callbacks, throttled in Rust.
- Avoid async foreign traits until a UniFFI release includes #2943. The `.v5` bindings target makes them compile today if one is needed.
- Use `async_runtime = "tokio"` only if a Rust `async fn` touching tokio is exported directly.
- The Swift wrapper re-exports DTOs and hides FFI types from SwiftUI views.
  - Pitfall: Xcode 26+ app targets default to `SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor`. A synchronous Rust call made from a view model runs on the main thread.
  - Package targets default to nonisolated unless `.defaultIsolation(MainActor.self)` is set. Keep the bridge package nonisolated.

## 2. XCFramework build

### Targets

- Use `aarch64-apple-darwin`, `aarch64-apple-ios` and `aarch64-apple-ios-sim`.
- No x86_64 slices. No `-macabi`, because the app is native macOS SwiftUI, not Catalyst.
- Add the targets once: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`.
- Set `MACOSX_DEPLOYMENT_TARGET=27.0` and `IPHONEOS_DEPLOYMENT_TARGET=27.0` during `cargo build`.

### Toolchain

- Add a `rust-toolchain.toml` that pins **1.99.0** with those 3 targets plus clippy and rustfmt.
- Floors:
  - UniFFI 0.31+ needs ≥ 1.87.
  - fsrs 6.6.2 is edition 2024 and depends on rand 0.10 (MSRV 1.85), so it needs ≥ 1.85.
  - burn 0.22 (not used, see below) needs 1.95.
- The installed 1.89 builds the prototype [measured]. Pinning to current stable avoids MSRV surprises from rusqlite, tokio and axum.

### fsrs 6.6.2 dependencies [doc + measured]

- **No `burn` at runtime.** `burn 0.17.1` is a **dev-dependency only**: tests and benches.
- Runtime dependencies: `ndarray 0.17`, `rayon 1.12`, `rand 0.10`, `itertools`, `serde`, `snafu`, `strum`, `log`, and `priority-queue =2.7.0` (LGPL-3.0+ OR MPL-2.0; choose MPL-2.0). The optional `rand_distr` is behind `experimental_cost_adr`.
- All pure Rust (ndarray uses matrixmultiply), so iOS targets have no backend or Metal concerns. Not compiled for iOS yet, because the iOS std targets are not installed. Expected to work: pure Rust plus rayon threads.
- `rayon` uses a global pool sized to the core count. Run the optimizer inside a bounded `ThreadPool` (2–4 threads) in a background task, never in the widget.

### Size [measured]

- The prototype `.a` (UniFFI + fsrs, fat LTO) is 20 MB before linking. This number is meaningless: it is mostly std objects.
- After linking and dead-stripping, a Swift executable using it is **0.77 MB, 0.58 MB stripped**.
- Expected full core (bundled SQLite, rustls/reqwest, zstd, axum or hyper for local servers, search and import): roughly **6–15 MB linked per binary** [estimate]. This is why the widget should not link it (§5).

### Release profile

- `lto = "fat"`, `codegen-units = 1`, `opt-level = 3`, `panic = "unwind"`.
- Use `strip = "debuginfo"` or none. Keep symbols in a dSYM; `dsymutil` runs at app link time.

### Tooling decision

- **Do not use `cargo-swift`.** 0.11.1 (2026-05) pins `uniffi_bindgen =0.31.1` and needs a matching cargo-swift for each UniFFI minor version. It has no 0.32 support, its configuration is interactive or CLI-only, and it generates its own package layout.
- **Use an in-workspace bindgen crate** (`fn main() { uniffi::uniffi_bindgen_swift() }`, `uniffi = { version = "=0.32.2", features = ["cli"] }`). It builds once in about 75 s.
- Use a ~100-line `scripts/build-core.sh`:
  1. Run `cargo build` for each target slice.
  2. Run `uniffi-bindgen-swift <lib.a> out --swift-sources`.
  3. Run `uniffi-bindgen-swift <lib.a> Headers/MeguriCoreFFI --headers`.
  4. Run `uniffi-bindgen-swift <lib.a> Headers/MeguriCoreFFI --modulemap --module-name MeguriCoreFFI --modulemap-filename module.modulemap`.
  5. Run `xcodebuild -create-xcframework -library <a> -headers Headers …`, once per slice.

### Pitfalls hit in the prototype [measured]

- Set `[bindings.swift] ffi_module_name = "MeguriCoreFFI"` (and `module_name`) in `uniffi.toml`, **and** pass `--module-name MeguriCoreFFI` to the modulemap step.
  - Without the flag, the modulemap declares `module meguri_core` while the Swift file does `import MeguriCoreFFI`, and the build fails.
- **Do not pass `--xcframework`** for a static-library XCFramework. It emits `framework module …`, which is only for `.framework` bundles.
- Put the header and `module.modulemap` in a **subdirectory** (`Headers/MeguriCoreFFI/`). A `module.modulemap` at the `Headers/` root collides with any other XCFramework's modulemap in the build products directory.
- Ship exactly **one Rust staticlib** in the app. It re-exports all Meguri crates. Two Rust staticlibs in one binary give duplicate std and runtime symbols.

### Static vs dynamic

- Use **static** (`crate-type = ["staticlib", "lib"]`): faster launch, dead-stripping, and no embedding or signing of an extra framework.
- Go dynamic only if several extensions must share the core and the size duplication matters (see §5).
- System linkage: rustls and native certificates, reqwest and DNS may need `Security`, `SystemConfiguration`, `CoreFoundation` and `libresolv`. Declare them as `linkerSettings` in `Package.swift`, or as `link_frameworks` in `uniffi.toml`.

### Docker

- Apple slices **cannot** be built in Linux Docker: they need the Apple SDK and linker. XCFramework builds run on the host.
- Keep every domain crate Linux-buildable: no Apple-only dependencies outside `cfg(target_vendor = "apple")`. Then `cargo test` and the Anki-oracle tests run in Docker per the user's preference.
- Optionally split crates:
  - pure domain crates with no UniFFI derives;
  - a thin `meguri_ffi` crate that holds the FFI DTOs and exports, and builds the staticlib.

  FFI shape changes then don't recompile the domain, and the domain stays free of FFI concerns.

## 3. Packaging into Xcode

### Layout (verified working with `swift build`, `swift test` and `xcodebuild test -destination 'platform=macOS'`)

```
Packages/MeguriKit/Package.swift      // swift-tools-version 6.2+, platforms macOS 27 / iOS 27
  .binaryTarget(name: "MeguriCoreFFI", path: "Binaries/MeguriCoreFFI.xcframework")   // gitignored, built by script
  .target(name: "MeguriCoreBindings", deps: [MeguriCoreFFI], swiftLanguageMode .v5)   // generated .swift only, gitignored
  .target(name: "MeguriKit", deps: [MeguriCoreBindings], .v6)                           // hand-written actor facade
  .testTarget(name: "MeguriKitTests")                                                   // Swift Testing
Packages/MeguriUI/...   (feature modules, depend on MeguriKit; MainActor default isolation)
Meguri.xcodeproj        (thin: app target(s) + widget + UI tests)
```

- **Agents can run Rust-backed Swift tests with plain `swift test` on macOS.** No Xcode project and no simulator are needed. This was verified with Swift Testing.
- SwiftPM build-tool plugins **cannot** run cargo to produce the binary target, because they are sandboxed and produce no binary targets. So Rust is built by the script **before** `xcodebuild` or `swift build`, never inside the package graph.

### Project file: commit `Meguri.xcodeproj` and use **folder-synchronized groups** (`PBXFileSystemSynchronizedRootGroup`, Xcode 16+)

- Adding, removing or moving Swift files needs no `project.pbxproj` edit.
- Keep targets few and stable:
  - the app as one multiplatform target (iOS + iPadOS + macOS destinations);
  - the widget extension;
  - the UI test bundle.
- Move build settings into `.xcconfig` files referenced by the project. Agents then edit text settings, not the pbxproj.
- Almost all code lives in local packages, whose `Package.swift` is ordinary text.
- The pbxproj then changes only when a target or capability is added. One agent does that, with Xcode MCP `XcodeNewTarget` / `AddEntitlement`, or by hand.

### Fallback: **XcodeGen 2.46.0** (2026-07-16)

- It supports `type: syncedFolder` and `options.defaultSourceDirectoryType: syncedFolder` since 2.44. Several synced-folder fixes landed in 2.45.x. A path fix is still unreleased (#1636).
- Switch to it, with the generated `.xcodeproj` gitignored, if parallel worktree agents keep conflicting on the pbxproj.
- Cost: a Homebrew tool dependency. Xcode must reload the project when it is regenerated while open for MCP.
- **Tuist** is heavier: a Swift-manifest DSL, its own CLI and cache, and fast-moving releases (4.213 RCs). Not justified for 3–4 targets.

### Command-line build and test

Run these after `scripts/build-core.sh`:

- `xcodebuild -project Meguri.xcodeproj -scheme Meguri -destination 'platform=macOS' build`
- `xcodebuild … -destination 'platform=iOS Simulator,name=iPhone 17 Pro,OS=27.0' test`
- Package scheme naming: a package with **one** library product gets the scheme `<Product>` (verified: `MeguriKit`). With several products, it is `<Package>-Package`.
- Xcode MCP tools (`BuildProject`, `RunAllTests`, `RenderPreview`) need Xcode running with the project open. They complement `xcodebuild` in a GUI session; headless CI cannot use them.
- Add a scheme **Build pre-action** that runs `scripts/build-core.sh --if-changed`. MCP-triggered builds then pick up Rust changes too.

## 4. Incremental dev loop

### `scripts/build-core.sh`

1. **Input hash short-circuit.** Hash the tracked Rust inputs: `crates/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `uniffi.toml`, the profile name and the slice list. If the hash equals `Binaries/.stamp`, exit in about 50 ms without invoking cargo. Cargo's no-op check costs about 0.3–1 s per slice.
2. **Output compare before replace.** Write the new `.a`, `.swift`, `.h` and modulemap to a temp directory. Replace a file only if its bytes differ.
   - This keeps mtimes stable, so Xcode doesn't recompile the ~1,300+ line bindings file or relink after a Rust edit that doesn't change the FFI surface.
3. **Slices by need:**
   - `mac` + `sim` by default: host `swift test` and simulator tests.
   - `device` only for device runs and archives.
   - An XCFramework without the destination's slice fails to link ("no library for this platform"). The script must accept `--slices` and the pre-action should derive it from `$PLATFORM_NAME`.

### Profiles

- Dev: `[profile.dev] opt-level = 1` plus `[profile.dev.package."*"] opt-level = 3`. Dependencies are optimized (ndarray and SQLite run 10–50× slower unoptimized); our code compiles fast and stays debuggable.
- A `release` (fat LTO) build of the core took **about 15 s from scratch** for UniFFI + fsrs [measured]. Expect several minutes once SQLite, rustls and axum are added. Use it only for performance gates and archives.
- All performance numbers (the 1 ms answer budget, frame timing) must come from **release** Rust **and** release Swift. Swift debug was 15× slower on the no-op call.

### Parallel worktrees

- Each worktree has its own `target/`, so a cold dependency build per worktree is the main cost.
- A shared `CARGO_TARGET_DIR` serializes concurrent cargo runs, because of the build-directory lock.
- Prefer `sccache` (or `CARGO_INCREMENTAL=0` + sccache for dependencies), or accept the cold builds.

## 5. Widget extension + App Group

- A widget **can** link the same SwiftPM product and therefore the static Rust lib. Each binary gets its own copy (app + appex), which duplicates about 6–15 MB.
- WidgetKit's memory ceiling (~30 MB, [2°], long-standing) leaves little room for opening the collection through Rust.
- **Recommendation:** the widget does **not** link the Rust core.
  - The app writes a small `WidgetSnapshot.json` (due counts, streak) into the App Group container whenever counts change and on going to the background, then calls `WidgetCenter.reloadTimelines`.
  - "Start review" uses `widgetURL` or an `OpenIntent` that opens the app.
  - Revisit only if an interactive widget must mutate the collection. Then move the core into one dynamic framework shared by the app and extensions.
- **App Group identifiers differ per platform:**
  - iOS needs `group.<id>`.
  - macOS 15+ grants silent access only to a Team-ID-prefixed group (`$(TeamIdentifierPrefix)<id>`), a profile-authorized `group.` id, or Mac App Store distribution. Otherwise it prompts, and extensions are silently denied [2°: Apple forums, DTS].
  - Use per-platform entitlements files, selected by `[sdk=macosx*]` in xcconfig. **Decide before first ship**: changing the group id moves the container and needs a migration.
- The collection DB lives in the App Group container only if an extension must read it. With the snapshot approach, keep it in the app's own Application Support. That also avoids `0xdead10cc` kills for holding SQLite locks in a shared container while suspended.
- macOS App Sandbox: the local Migaku and AnkiConnect servers need `com.apple.security.network.server`. Sync needs `network.client`.

## 6. Testing

- **Swift Testing** (`@Test`, `#expect`) for all package unit and integration tests. It is the Xcode 27 default and runs under both `swift test` and `xcodebuild test` [measured: "TEST SUCCEEDED" via xcodebuild with the macOS destination].
- **XCTest** only for XCUITest and `measure(metrics:)` performance tests (`XCTOSSignpostMetric`, `XCTHitchMetric`). Swift Testing has no performance metrics.
- Package tests on the simulator: from `Packages/MeguriKit`, run `xcodebuild test -scheme MeguriKit -destination 'platform=iOS Simulator,name=iPhone 17 Pro,OS=27.0'`.
  - Create the device first (see §0).
  - This needs the `aarch64-apple-ios-sim` slice in the XCFramework.
- Rust unit tests and oracle tests run with `cargo test` in Docker on Linux, so the domain crates must stay Linux-buildable. FFI-boundary tests run on the host through `swift test`.

## Decision-changing findings (summary)

1. fsrs 6.6.2 has **no burn runtime dependency**: it is ndarray + rayon only. The iOS build risk and the binary-size worry about burn disappear.
2. UniFFI 0.32.2 **async foreign traits do not compile in Swift 6 mode** (the fix is unreleased), and Swift task cancellation doesn't reach Rust. Design the FFI as sync calls + sync callbacks + explicit cancel tokens, and compile the generated bindings in a `.v5` target.
3. Do not use `cargo-swift`: it is stuck on UniFFI 0.31. Use an in-repo `uniffi-bindgen-swift` pinned `=0.32.2` and a hashing build script.
4. The widget should not link the Rust core: use a JSON snapshot in the App Group. App Group ids must be per-platform (Team-ID prefix on macOS), decided before first ship.
5. No foldable-iPhone simulator and no default "iPhone 17 Pro" simulator exist in Xcode 27.0. Test plans must create devices with `simctl`, and foldable checks need hardware or a size-class approximation.

## References

- UniFFI changelog v0.32.2: https://github.com/mozilla/uniffi-rs/blob/v0.32.2/CHANGELOG.md
- UniFFI Swift docs (overview, configuration, uniffi-bindgen-swift): https://github.com/mozilla/uniffi-rs/tree/v0.32.2/docs/manual/src/swift
- Async foreign trait Swift 6 error: https://github.com/mozilla/uniffi-rs/issues/2929 (fix PR #2943, unreleased); https://github.com/mozilla/uniffi-rs/issues/2458; cancel: https://github.com/mozilla/uniffi-rs/pull/2887
- crates.io API: fsrs 6.6.2 dependencies; burn 0.22.0 (MSRV 1.95); cargo-swift 0.11.1 (uniffi_bindgen =0.31.1); swift-bridge 0.1.59; boltffi 0.31.0
- BoltFFI benchmark table (vendor): https://docs.rs/boltffi_cli
- XcodeGen changelog (syncedFolder): https://github.com/yonaskolb/XcodeGen/blob/master/CHANGELOG.md
- macOS 15 app group rules: https://developer.apple.com/forums/thread/763826 ; https://developer.apple.com/documentation/xcode/configuring-app-groups
