# SwiftUI app architecture for Meguri (macOS / iOS / iPadOS 27 + foldable)

Researched 2026-10-12 against the local Xcode 27.0 install (27A266a; Swift 6.4; macOS 27.0 and iOS 27.0 SDKs; iOS 27.0 simulator runtime now installed), Apple DocC through the Xcode MCP `DocumentationSearch` tool, and a few web sources.

Labels:
- **[SDK]** = read in the 27 SDK `.swiftinterface` or headers.
- **[Doc]** = Apple DocC.
- **[Web]** = a secondary source.
- **[Spike]** = must be proved with a prototype before the plan depends on it.

Builds on and corrects `local-data/research/10-apple-platform.md` §2.13 and §3, and 15/15c/15d. Requirement IDs refer to the plan doc.

---

## 0. Decisions that change the plan (read first)

1. **Card faces: use `WKWebView` through `UIViewRepresentable`/`NSViewRepresentable`, not SwiftUI `WebView`/`WebPage`.** This reverses doc 10 §2.13. `WebPage` is fine for simple read-only HTML, but the 27 SDK has no public way to get at its `WKWebView`. It is missing four things the requirements need [SDK]:
   - **Process-termination callback (R454, R538, AE20).** There is no `webViewWebContentProcessDidTerminate` equivalent. A crash shows up only as `WebPage.NavigationError.webContentProcessTerminated` on a navigation sequence, and the docs do not say what happens to an idle page whose process dies.
   - **Page zoom (R339 "card zoom").** There is no `pageZoom`. `WKWebView.pageZoom` exists (macOS 11 / iOS 14).
   - **Autoplay policy.** There is no `mediaTypesRequiringUserActionForPlayback`. Without it, iOS blocks `play()` that a card script calls after a *native* reveal tap, because the web page saw no user gesture.
   - **File loading.** There is no `loadFileURL(_:allowingReadAccessTo:)`, `takeSnapshot(with:)` or `obscuredContentInsets`/`underPageBackgroundColor`.

   Everything else needed is the same in both APIs: scheme handlers, `WKUserContentController`, `callAsyncJavaScript`. So the cost of the wrapper is small. Keep a single `CardWebView` component, and use it for review, previews, the browser's Question/Answer columns and the template-editor preview.
2. **Keep the collection out of the App Group container.** R1433/R317 already say the widget reads only a JSON snapshot. Store the collection, media and backups in the app's own Application Support. Put only the snapshot in the App Group. Then the `0xdead10cc` "file lock in a shared container while suspended" risk (doc 10 §3.6) does not apply to SQLite. R14 still needs commit-on-background, but no longer needs a lock-release dance. This reverses doc 10 §3.6.
3. **No foldable APIs in the 27.0 SDK.** `ArrangementView` and `ReservedRegion` are absent from the iOS 27.0 SwiftUI and UIKit interfaces, and `simctl list devicetypes` shows no foldable device [SDK]. R9, R10, R576 and AE7 cannot be built or tested until the Xcode 27.1 SDK ships. **Build the layout switch on `horizontalSizeClass` now (R2).** Add fold-specific code (the reserved region, arrangement) as a later unit gated on 27.1.
4. **Mac: a single `Window` scene quits the app when its window closes** [Doc: "If your app uses a single window as its primary scene, the app quits when the window closes"]. R11 needs the app to stay alive while the Migaku or AnkiConnect servers are on. Use `Window` (R286 also wants the free "Window > Meguri" item) plus an `NSApplicationDelegateAdaptor` whose `applicationShouldTerminateAfterLastWindowClosed` returns `!integrationsEnabled`. **[Spike]**: confirm the delegate overrides `Window`'s built-in quit. The fallback is `WindowGroup` + `.commandsRemoved()` + suppressing ⌘N.
5. **Background work on iOS: there are three task kinds, with different rules** [SDK]:
   - **FSRS auto-optimize (R1850, AE28).** Use `BGProcessingTaskRequest` (`requiresExternalPower` optional). The SwiftUI scene modifier `.backgroundTask(.processingTask("id"))` is **new in iOS 27**.
   - **One-way full sync, large media download, user-started import.** Use `BGContinuedProcessingTask` (iOS 26, iOS only). It must be **user-initiated**, shows system UI with title and subtitle, **must report `NSProgress`**, and stalled tasks are expired. Never use it for automatic work.
   - **Automatic sync while backgrounded (R1552, R1553).** Use `UIApplication.beginBackgroundTask` on leaving, plus `BGAppRefreshTask` (`.backgroundTask(.appRefresh("id"))`).
   - **macOS has none of these** (BackgroundTasks is `API_UNAVAILABLE(macos)`). Use `NSBackgroundActivityScheduler` and `ProcessInfo.beginActivity`.
6. **Doc 10 had a wrong API name.** It is `toolbarMinimizationBehavior(_:for:)`, not `toolbarMinimizeBehavior`, and it comes with `toolbarMinimizationSafeAreaAdjustment` and `toolbarMinimizationRestoration` (27) [SDK]. The tab-bar version is `tabBarMinimizeBehavior(_:)`.
7. **Mac App Group ID must use the team-ID prefix** (`<TEAMID>.net.meguri.shared`). iOS uses `group.net.meguri.shared`. A `group.`-prefixed ID on macOS 15+ can make the system prompt "would like to access data from other apps" on every launch, and can break widgets [Web, several dev-forum threads]. Use per-platform entitlements and resolve the ID from Info.plist at runtime.
8. **Xcode MCP is now enabled in this environment.** `mcp__xcode__DocumentationSearch` answered in this session. Doc 15 §0.2 said MCP was off, so `RenderPreview` should now be usable for the inner loop.

---

## 1. Card rendering (WebKit)

### 1.1 What the 27 SDK offers [SDK]

| Need | `WebPage` (SwiftUI) | `WKWebView` | Verdict |
|---|---|---|---|
| Custom scheme handler | `Configuration.urlSchemeHandlers[URLScheme]`, async `reply(for:)` → `URLSchemeTaskResult` | `setURLSchemeHandler(_:forURLScheme:)` | Both fine |
| JS→native | `Configuration.userContentController` (a `WKUserContentController`) | same | Both fine. Use `WKScriptMessageHandlerWithReply` so JS gets a Promise back. |
| Native→JS | `callJavaScript(_:arguments:in:contentWorld:)` | `callAsyncJavaScript(_:arguments:in:contentWorld:)` | Both fine |
| Process crash notice | only `NavigationError.webContentProcessTerminated` while navigating | `webViewWebContentProcessDidTerminate(_:)` | **WKWebView** |
| Page zoom | none | `pageZoom` | **WKWebView** |
| Autoplay policy | iOS has only `mediaPlaybackBehavior` (inline/fullscreen) | `mediaTypesRequiringUserActionForPlayback`, `allowsInlineMediaPlayback` | **WKWebView** |
| File URLs | none | `loadFileURL(_:allowingReadAccessTo:)` | WKWebView (option) |
| Snapshot for tests | `exported(as: .image(region:…, afterScreenUpdates:))` | `takeSnapshot(with:)` | Both |
| Insets and background | `webViewContentBackground(.hidden)` | `obscuredContentInsets` (26), `underPageBackgroundColor`, `scrollView` (iOS) | WKWebView gives more control |
| `suppressesIncrementalRendering` | yes | yes | Keep **false** |
| Content mode (R477, iPad mobile UA) | `NavigationPreferences.preferredContentMode = .mobile` | `WKWebpagePreferences.preferredContentMode` | Both |

New in 27 that is useful [SDK]:
- `WKUserContentController.addBuffer(_:name:contentWorld:)` / `removeBuffer`. It exposes bytes as `window.webkit.buffers.<name>`, one world only. Possibly useful for big per-card payloads (dictionary JSON) without string-encoding them through `callAsyncJavaScript`.
- `WKContentWorld(configuration:)` with `WKContentWorld.Configuration` (`isInspectable`, `jsHandleCreationEnabled`, `nodeSnapshotCreationEnabled`).
- `WKJSHandle` (keeps a JS object alive across calls).

`WKProcessPool` is deprecated (one shared pool). Nothing is gained by sharing configurations to "share a process".

### 1.2 Two alternating pre-rendered pages (R443–R450, R465–R468, R1807)

- **Ownership.** A `ReviewRenderer` (MainActor class, owned by the review session model, *not* by a view) creates two `WKWebView`s at launch (R329) with one shared configuration:
  - the same `WKUserContentController` instance, so messages from both pages reach one handler; tell the pages apart with `message.webView` and `frameInfo`
  - the same scheme-handler instance
  - `websiteDataStore = .nonPersistent()`

  The `Representable` only *hosts* an existing web view: `makeUIView` returns a container, and the web view is added as a subview. This survives a layout switch between tab and sidebar layouts and a fold/unfold without reloading (R5, R576). Reparenting a `WKWebView` does not reload it.
- **Keep the idle page rendering.** WebKit throttles or stops rendering and `requestAnimationFrame` when a web view is hidden or not in a window [Web: dev-forum reports, WebKit bugs; no Apple doc]. So:
  - Keep **both web views in the window, full frame and opaque**, stacked. The idle one sits *beneath* the visible one.
  - Never use `isHidden`, `.hidden()`, conditional removal, or an off-screen frame.
  - Reveal by changing z-order (or the opacity of the front view) inside one `CATransaction`.

  **[Spike S1]** Prove that (a) scripts, fonts and images fully settle in the occluded page, (b) the swap is committed in the input's frame at 120 Hz, and (c) no white flash appears.
- **Script persistence semantics (R468).** Card scripts reference `onUpdateHook`/`onShownHook` and page globals, and call `pycmd`-style shims. So **the runtime that inserts sides must run in `.page` world.** Give the message handler only an allowlist of card-scoped actions (R2038). A separate client world helps only for Meguri's own probes, such as tests and first-paint measurement.
- **Recovery (R454).**
  - On `webViewWebContentProcessDidTerminate`, rebuild that page: load the skeleton, re-render the current side, restore `#typeans`.
  - Each page has its own WebContent process, so the idle page can die on its own. Re-render it lazily.
  - On iOS, termination often happens while the app is backgrounded. Check `isLoading`/`url == nil`, or ping each page with `callAsyncJavaScript` on `scenePhase == .active`.
- **Measuring "visible" (R1861/R1862).**
  - For a pre-rendered swap, painting already happened in the idle page. The visible moment is the native commit. Signpost from input delivery to the `CATransaction` completion or the next `CADisplayLink` tick after the swap. Then have the front page confirm with `requestAnimationFrame(() => requestAnimationFrame(post))`, sent back over the reply handler, and record both.
  - For on-demand renders (R446): the end of the interval is the double-rAF message after `#qa` insertion and the `onShownHook` pass.
  - Safari 26.2+ has LCP and Event Timing [Web], but they are per navigation or per web input. They do not help with `innerHTML` swaps after a native tap.
  - Private `_doAfterNextPresentationUpdate` is off-limits.

### 1.3 Media via a custom scheme (R449, R1814)

- Register `meguri-media:` (or similar; never http, https or file) before creating the web view. Serve from disk in chunks, never whole files in memory.
- **Range requests.** Handle both ranged requests (reply `206` with `Content-Range`, `Accept-Ranges: bytes`, `Content-Length`) and unranged ones (`200`). A WebKit bug about `<video>` over custom schemes on iOS arriving without `Range` was fixed in 2021 [Web: webkit.org/b/203302]. **[Spike S2]**: test `<audio>`/`<video>` seek on an iOS 27 device and on a Mac.
- **Recommended split.** Play Anki `[sound:]` tags **natively** (AVFoundation), as Anki and AnkiMobile do. Only template-authored `<audio>`/`<video>` goes through WebKit. On iOS, set `mediaTypesRequiringUserActionForPlayback = []` so card scripts can play after a native tap.
- **Fallback if S2 fails.** Use `loadFileURL(_:allowingReadAccessTo:)` with a read-access root that contains both a runtime directory and the media directory. The cost: a `file://` origin, which breaks some `fetch()` uses by templates. Custom-scheme `fetch()` from the same custom origin is reported to hit CORS checks in some versions [Web]. Spike that too.

---

## 2. Navigation and windows

- **Layout switch (R2–R5, R17).** Branch on `@Environment(\.horizontalSizeClass)` (iOS/iPadOS). macOS has no size class, so the Mac always uses the sidebar layout with a minimum window width.
  - **Compact:** `TabView` with `Tab`s, bottom tabs on iPhone, a `NavigationStack` per tab.
    - `Tab(role: .search)` is available.
    - `Tab(role: .prominent)` is new in 27 [SDK, `anyAppleOS 27`].
    - `tabBarMinimizeBehavior(.onScrollDown)` and `tabViewBottomAccessory` are available.
  - **Regular / Mac:** `NavigationSplitView`. The sidebar `List` has the sections (Home, Browse, Stats, Note Types, Settings) and a "Decks" `Section` with an `OutlineGroup`/`DisclosureGroup` tree.
  - **Do not use `.tabViewStyle(.sidebarAdaptable)` for this.** Its sidebar shows `TabSection` → `Tab`, only two levels, so it cannot hold an arbitrarily deep deck tree. Its tab/sidebar toggle is a *user* choice on iPad, not width-driven. The HIG says to use `NavigationSplitView` when the sidebar must not convert to a tab bar.
  - Keep one `@Observable AppNavigation` model (section, selected deck ID, per-section path, browser search/selection, editor drafts) *above* the branch. Both layouts then render from it, and switching keeps state (R5).
- **Mac scene.** `Window("Meguri", id: "main")` with `.windowResizability(.contentMinSize)`, `.defaultSize`, `.restorationBehavior(.automatic)` (R12), and `.commands { … }`.
  - Do not add a `Settings` scene (R6). Map ⌘, through `CommandGroup(replacing: .appSettings)` to the Settings section.
  - Add an `NSApplicationDelegateAdaptor` for R11 and for quit-time sync (`applicationShouldTerminate` → `.terminateLater` while the progress sheet runs, R127).
- **iPad scene.** `WindowGroup` with `UIApplicationSupportsMultipleScenes = NO` (R7).
- **Commands and shortcuts (R278, R339, R1231).**
  - Add `CommandMenu`s and `CommandGroup`s, and route them to the active section with `@FocusedValue`/`.focusedSceneValue`. Enable an item only when its focused value exists.
  - `.keyboardShortcut` items show in the Mac menu bar and the iPad shortcut overlay.
  - **Pitfall:** unmodified shortcuts (Space, 1–4) can steal keys from a focused `<input>` in the card (`#typeans`). The web page must report focus changes over the bridge, and the review commands must disable themselves while an input is focused. **[Spike S3]** on Mac and on iPad with a hardware keyboard.
- **New 27 toolbar and list APIs** [SDK]:
  - `ToolbarItem(placement: .topBarPinnedTrailing)`, `visibilityPriority`, `toolbarMinimizationBehavior(_:for:)`
  - `.reorderable()`, `.swipeActionsContainer()`
  - `alert(error:)`
  - `TabSection.defaultSectionExpansion`

---

## 3. State and concurrency (R15, R362, R1817, R1856–R1860)

- **Isolation defaults.** App and UI packages use MainActor by default (`SWIFT_DEFAULT_ACTOR_ISOLATION = MainActor`, or `.defaultIsolation(MainActor.self)` in Package.swift). The **`CoreBridge` package stays nonisolated by default**, so generated UniFFI code and wrappers never inherit MainActor.
- **Rust calls never run on the cooperative pool or the main thread.** Long synchronous FFI calls would starve the cooperative pool. Use actors with **custom serial executors backed by `DispatchSerialQueue`**:
  ```swift
  actor CoreLane {
    let queue = DispatchSerialQueue(label: "core.review", qos: .userInteractive)
    nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
  }
  ```
  (`_DispatchSerialExecutorQueue: SerialExecutor` [SDK].)
  - Use separate lanes: **review/interactive** (next card, answer commit, render), **query** (browser search, stats; cancellable), and **maintenance** (sync, import, optimize, check database).
  - The Rust core must be thread-safe (`Send + Sync`) and own its own SQLite connection(s) and locks. The lanes give ordering and QoS, not correctness.
  - `DispatchQueue` also conforms to `TaskExecutor` [SDK], so `withTaskExecutorPreference` can pin `nonisolated async` helpers.
- **`@concurrent`** (SE-0461) is for one-off `nonisolated async` helpers. Do not mark Rust wrappers `@concurrent` and call them synchronously. Swift 6.4 adds `withTaskCancellationShield` [SDK], which is useful for "commit answer even if the view task is cancelled".
- **Observable models.** `@Observable @MainActor` view models hold value snapshots: the deck list, the current card's pre-rendered HTML, counts.
  - The core returns whole pages or snapshots per call (R1859). The UI never calls the core in `body`.
  - Progress reaches the UI as an `AsyncStream` with `.bufferingNewest(1)`, sampled at about 10 Hz (R1858).
- **Answer path (R362/R1817).** The UI swaps pages at once. The commit goes to the review lane as a fire-and-forget `Task` held in a session-owned queue. Failure re-presents the card (R362).
- **UniFFI caveat** (doc 10): async bindings are not `Sendable`-clean under Swift 6. Prefer *sync* UniFFI exports wrapped by the lane actors. Use Rust-async (tokio) only inside the core for sync and HTTP, and report results through callback interfaces or polling.
- **Hang guard.** Add `OSSignposter` intervals on every bridge call (R1861). In debug builds, assert `!Thread.isMainThread` inside the bridge.

---

## 4. Liquid Glass and accessibility [SDK]

- **Glass APIs:**
  - `glassEffect(_ glass: Glass = .regular, in: some Shape)`
  - `Glass.regular / .clear / .identity`, `.tint(_:)`, `.interactive(_:)`
  - `GlassEffectContainer(spacing:)`, `glassEffectID`, `glassEffectUnion(id:namespace:)`, `glassEffectTransition(.matchedGeometry/.materialize/.identity)`
  - `.buttonStyle(.glass)`, `.glass(_ glass:)`, `.glassProminent`
  - `backgroundExtensionEffect()`, `scrollEdgeEffectStyle(_:for:)`, `scrollEdgeEffectHidden`, `safeAreaBar(edge:)`, `ToolbarSpacer`, `sharedBackgroundVisibility(_:)` on toolbar items
- **Rules for Meguri (R176, R1996).**
  - Use system bars only.
  - The answer bar goes in `safeAreaBar(edge: .bottom)` or a bottom toolbar, as `.glass` buttons inside one `GlassEffectContainer`.
  - The card web view is content: no glass, `webViewContentBackground`/opaque.
  - `Glass.identity` is the test switch to disable glass in snapshot runs (doc 15c).
- **Environment.**
  - `accessibilityReduceTransparency`, `accessibilityReduceMotion`, `colorSchemeContrast`, `accessibilityDifferentiateWithoutColor`, `dynamicTypeSize`, `appearsActive`, and `systemPrefersReducedResourceUsage` (new 27).
  - Underscored setters (`\._accessibilityReduceMotion` and so on) exist for previews and snapshot tests. They are unstable SPI-ish, so use them in test targets only.
- **Card web view.** Pass Dynamic Type, Reduce Motion and Increase Contrast into the page natively, as `<html>` classes or CSS variables (R475 sets classes natively). Do not rely on `-apple-system-body` alone.

---

## 5. Widgets (R306–R318, R1431–R1438) [SDK]

- **Families:** `systemSmall/Medium/Large/ExtraLarge`. `systemExtraLargePortrait` is new in 27 (not required).
- **Rendering modes:** `widgetRenderingMode`, `widgetAccentedRenderingMode(_:)`, `showsWidgetContainerBackground` for R313/R1437.
- **Writing the snapshot.** The app writes `snapshot.json` atomically (write to a temp file, then `replaceItemAt`) into the App Group. It then calls `WidgetCenter.shared.reloadTimelines(ofKind:)` only when the bytes changed (R1434).
- **Timeline.** The provider returns entries for now plus each upcoming rollover, with `.after(nextRollover)` or `.atEnd` (R1435). The widget never touches SQLite (R317).
- **Entitlements.** The widget extension needs the same App Group entitlement. macOS uses the team-ID-prefixed ID (§0.7), and the Mac widget extension must be sandboxed.

---

## 6. Background and lifecycle

- **iOS leaving the app** (R14, R123, R452, R1434, R1552):
  - `scenePhase` `.inactive`/`.background`: pause timers, stop audio, save the session, write the widget snapshot, start the incremental sync inside `beginBackgroundTask`, and checkpoint the WAL.
  - **On the way back** (R453/R454): verify both pages are alive.
- **Mac leaving the app:**
  - Use `NSApplication.didResignActive` and `willTerminate` from the delegate adaptor. `scenePhase` on a Mac `Window` mostly tracks visibility, not activation.
  - Use `ProcessInfo.beginActivity(options: .userInitiated)` during sync and import to avoid App Nap.
  - Use `NSBackgroundActivityScheduler` (interval, tolerance, `qualityOfService = .utility`) for FSRS optimization.
- **Info.plist (iOS).** `BGTaskSchedulerPermittedIdentifiers` must list the processing and refresh IDs and the continued-processing wildcard (`<bundle>.continued.*`).

---

## 7. Testing and verification

- **Swift Testing** for unit and logic tests. **UI and performance tests stay XCTest** [Doc: "Continue to use XCTest for user interface tests and performance tests"]. `XCUIApplication` lives in the separate **XCUIAutomation** framework [SDK].
  - Use `performAccessibilityAudit(for:_:)`.
  - `XCUIDevice.shared.voiceOverService` is new in 27.
  - Use `XCTHitchMetric(application:)`, `XCTOSSignpostMetric(subsystem:category:name:)` and `XCTApplicationLaunchMetric`. All exist [SDK].
- **Snapshots.** swift-snapshot-testing supports Swift Testing via `@Suite(.snapshots(record: .failed))` and `withSnapshotTesting(record:)` [Web: README].
  - Glass needs an app-hosted test target plus `drawHierarchyInKeyWindow` (doc 15c).
  - **Workaround needed for issue #1125** (iOS 27 safe-area shift).
  - Snapshot *card HTML* as strings (deterministic), and web views via `takeSnapshot` only in end-to-end runs.
- **Deterministic launch.** Use `launchArguments`/`launchEnvironment` for:
  - fixture collection path
  - fixed "now" injected into the core
  - animations off
  - locale and time zone
  - `-ReduceMotion`/`-ReduceTransparency` overrides, which the app maps onto the underscored env values (no `simctl` switch exists for these)
- **Simulator CLI** [SDK/local]:
  - `simctl status_bar <dev> override --time … --batteryState … --wifiBars …`
  - `simctl ui <dev> appearance|increase_contrast|content_size`
  - `simctl io <dev> screenshot` and `recordVideo`
  - `devicectl device settings appearance` has `--reduce-motion`, `--reduce-transparency`, `--liquid-glass-opacity`, `--look-and-feel`.
  - `devicectl list devices` lists simulators ("Reality: simulated"). Whether the appearance settings apply to simulators is **[Spike]**.
  - The iOS 27.0 runtime and iPhone 17/18 Pro and iPad Pro M5 device types are installed; there is still no foldable device type.
- **RenderPreview / Xcode MCP.** Available in this session (§0.8). It needs Xcode open with the project; use it for the inner loop only.

---

## 8. Mac sandbox and loopback servers (R1663–R1675, R2041)

- **Entitlements:** `com.apple.security.app-sandbox`, `com.apple.security.network.server` (listen) and `com.apple.security.network.client` (sync).
- **Local network prompt.** TN3179 [Doc]: "Listening for and accepting incoming TCP connections" does **not** require local network access. A local network means broadcast-capable interfaces, and loopback is not one. Binding only to `127.0.0.1`/`::1` should therefore never prompt.
- **Firewall.** Whether the Application Firewall ignores loopback-only listeners is unverified. **[Spike]** on a clean user account, as R1671 already requires.
- **Lifecycle.** The servers live in the app process (R1666). Hence the R11 window-close behaviour in §0.4.

---

## 9. Spikes to schedule before the dependent units

| ID | Proves | Blocks |
|---|---|---|
| S1 | The occluded idle `WKWebView` fully renders; z-order swap in the input frame at 120 Hz; no flash; double-rAF confirmation works | Review renderer, R1807 |
| S2 | Custom-scheme `<audio>`/`<video>` with Range/206 and seeking on an iOS 27 device and on a Mac; `fetch()` from the card to the same scheme | Media handler, template compatibility |
| S3 | Unmodified menu shortcuts vs a focused `#typeans` on Mac and iPad | Review commands |
| S4 | `Window` scene + `applicationShouldTerminateAfterLastWindowClosed` keeps the app alive | R11 |
| S5 | `devicectl` appearance flags on simulators | Snapshot matrix R1971 |
| S6 | Foldable size-class transitions on the 27.1 SDK; `ReservedRegion`/`ArrangementView` | R9, R10, R576 (deferred) |

## References
- Local SDK: `MacOSX.sdk/System/Cryptexes/OS/System/Library/Frameworks/WebKit.framework/.../WebKit.swiftmodule/arm64e-apple-macos.swiftinterface`; `_WebKit_SwiftUI.swiftmodule`; `WebKit.framework/Headers/{WKWebView,WKWebViewConfiguration,WKUserContentController,WKContentWorldConfiguration,WKJSHandle}.h`; iOS `SwiftUI.swiftmodule`/`SwiftUICore.swiftmodule`; `BackgroundTasks.framework/Headers/BGTask*.h`; `WidgetKit.swiftmodule`; `XCUIAutomation.framework`; `Dispatch.swiftmodule`.
- DocC (Xcode MCP): SwiftUI `Window` ("app quits when the window closes"), XCTest overview (UI/perf stay XCTest), WebPage navigation docs.
- TN3179: https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy
- WebKit custom-scheme media Range bug: https://webkit.org/b/203302 ; forum: https://developer.apple.com/forums/thread/744322
- WKWebView offscreen rendering reports: https://developer.apple.com/forums/thread/710015 , https://bugs.webkit.org/show_bug.cgi?id=242312
- macOS App Group prefix: https://developer.apple.com/forums/thread/763826 , https://mjtsai.com/blog/?p=44819
- Safari 26.2 LCP/Event Timing: https://caniuse.com/mdn-api_largestcontentfulpaint
- swift-snapshot-testing README: https://github.com/pointfreeco/swift-snapshot-testing
