# Spike S2: the custom-scheme media origin

Date: 2026-10-16. Unit: U65. Toolchain: Xcode 27.0 (27A266a). Hosts: this Mac (Mac15,9, M3 Max, macOS 27.0.1 26A434), and the test VM `meguri-mac` (macOS 27.0.1 26A434), whose pool simulator Meguri-Pool-iphone-1 is an iPhone 17 Pro on iOS 27.0 (24A434). Status: S2(b) to (f) passed. S2(a) passed on the Mac, for `<audio>` and `<video>`, and on the iPhone simulator for `<video>`; it is **pending on an iOS 27 device for both `<audio>` and `<video>`**, which was unavailable on 2026-10-16 (the 2026-10-20 signing attempt is recorded below), so the configuration below is not yet confirmed for iOS. Question (KTD11, swiftui-arch §1.3): can a `meguri-media:` scheme handler serve card media with Range, `fetch()`, module imports, fonts and persistent storage, and keep scripts in media documents from running (R493–R503, R560)? If S2(a), (b) or (d) failed, U66 and U67 would take the `loadFileURL` fallback.

## What was run

- `app/Packages/MeguriKit/Tests/CardRenderingTests/Spikes/CustomSchemeMediaSpikeTests.swift` on this Mac (`tools/mg swift-test --package MeguriKit`): WKWebView in an off-screen window, a throwaway handler (`Spikes/Support/SpikeMediaHandler.swift`) that serves in-memory files at the root of the origin, answers one `Range` with 206 and logs every request. Media is made at run time: a silent constant-bitrate MP3 (MPEG-1 Layer III, 128 kbit/s, 8,388,789 bytes, 525.5 s), an H.264 MP4 (20 s, fast start), and system faces rebuilt as standalone sfnt files.
- `app/MeguriUITests/CardSwapTimingSpikeTests.swift` (`testVideoSeeksToNinetyPercentThroughRangeRequests`, `testStorageOnTheMediaOriginSurvivesARelaunch`) through `tools/mg perf-ui` in the VM, against the test-hook routes `spike/media-seek` and `spike/storage/{write,read}` (`app/Meguri/TestHooks/MediaOriginSpike.swift`). `testAudioSeeksToNinetyPercentThroughRangeRequests` (route `spike/media-seek/audio`, the same 8 MB silent MP3) is the device check: it skips wherever `kern.hv_vmm_present` does not say real hardware, so in the VM. Each media check fails on any ranged answer that is not a 206 with the `Content-Range` asked for, carrying exactly those bytes of the file. The range those checks expect comes from the in-app handler's own parser, so `app/MeguriSnapshotTests/TestHooks/MediaOriginSpikeHandlerTests.swift` holds the handler to answers worked out by hand on a 10-byte file: bounded (`bytes=2-5`, `0-0`), bounded past the end (`8-20`), open-ended (`7-`, `0-`), suffix (`-3`, `-20`) and no `Range` (200), each with its status, `Content-Range`, `Content-Length` and body. With a suffix off by one byte planted in the parser, three of its expectations failed while the in-app check still passed the answers (`tools/mg snapshots --platform iphone`, 20261016T084152Z-27964).

## Results

| Item | macOS host | iPhone 17 Pro simulator (VM) |
|---|---|---|
| (a) `<audio>` 8 MB MP3 seek to 90 % and play | pass: seeked to 472.95 s of 525.5 s, played to 473.85 s; ranged requests `0-1`, `0-7`, `0-65535`, `8388661-8388663`, `7549368-7614903`, `7614904-7680439`, each answered 206 with `Content-Range: bytes a-b/8388789` and that many bytes | not checkable: metadata loads through ranged 206 answers, then `play()` never settles and the seek never completes. The same bytes from a blob URL stall the same way: the VM's simulator has no audio route, and an audio element left loading there once stalled the app's main thread for a minute, so the audio test skips the VM. **Pending on an iOS 27 device** (runbook below). |
| (a) `<video muted>` H.264 seek to 90 % and play | pass: about 70 ranged requests, all 206 | pass: seeked to 18.0 s of 20 s, played to 18.42 s; 97 ranged requests answered 206, 13 of them in the last fifth |
| (b) `import('./_lib.js')` and `fetch('_data.json')` from the card page | pass (`text/javascript`, `application/json`, `Access-Control-Allow-Origin: *`) | not run separately |
| (c) `url("/_root.ttf")`, `url("_relative.ttf")`; `plain.png`, `./dotted.png`, `/rooted.png` from a page under `/_anki/` with `<base href="meguri-media://media/">` | pass: every URL requested at the root, both faces loaded, all images decoded | not run separately |
| (d) localStorage and IndexedDB in `WKWebsiteDataStore(forIdentifier:)` | pass: a later page with a new store object for the same identifier reads both values; another identifier reads neither | pass across an app relaunch: written, app terminated, read back equal from both |
| (e) script in SVG and HTML media opened as documents | pass with `Content-Security-Policy: script-src 'none'` on those responses: no script ran in an SVG iframe, an HTML iframe or a top-level SVG; `<script src="_x.js">` in the card ran | not run separately |
| (f) origin forms | `meguri-media://media/` (origin `meguri-media://media`), `meguri-media:///` (origin `meguri-media://`) and `meguri-media://Media.Local/` (origin `meguri-media://media.local`) all fetched, imported and kept localStorage and IndexedDB | `meguri-media://media/` used for (a) and (d) |

In the macOS VM the same app checks passed: the video seek (112 ranged answers, 20 in the last fifth; 114 and 24 in perf-ui 20261016T091331Z-419) and storage across a relaunch. The video check passed on the iPhone simulator in three runs (97, 99 and 99 ranged answers; the last, perf-ui 20261016T084357Z-34916, with the byte comparison: seeked to 18.0 s, played to 18.42 s, 14 ranged answers in the last fifth).

Red runs before the handler did Range and CSP (same tests, handler with both off): the MP3 and the MP4 still played, because WebKit read whole files answered with 200 to ranged requests, but no ranged answer existed, so (a) failed; and the SVG and HTML scripts ran, so (e) failed.

## Decision

- Configuration: U67 serves card pages and media from a `WKURLSchemeHandler` on `meguri-media:`, as below; `loadFileURL(_:allowingReadAccessTo:)` is not taken. S2(b) and (d) passed everywhere they ran, and (a) on the Mac. On iOS the configuration stands, until the device check runs, on the simulator's video seek and on WebKit sending audio the same ranged requests up to the point the VM stalls: it is confirmed for iOS only when both device seeks pass.
- If either device seek fails (no seek, no playback past the target, or no ranged request near it), S2(a) has failed on iOS: stop U67's media work and escalate, with the fallback KTD11 names (`loadFileURL` with a read-access root) and its costs to be measured then by re-running S2(b), (d) and (e) on file URLs.
- Origin form: `meguri-media://media/`. A fixed lower-case host gives a tuple origin (`meguri-media://media`); the empty-host form also worked on 27.0, but its origin (`meguri-media://`) is an edge of the URL standard, and host case folds (`Media.Local` became `media.local`). The host never changes (R493).
- The card page document's URL is the origin root, `meguri-media://media/` (amended by U66 on 2026-10-18; this record first put pages under `/_anki/`). WebKit resolves `import()` in an inline script against the document's URL, not against `<base href>`, so a page at `/_anki/page.html` sends `import("./_m.js")` to `/_anki/_m.js` (R520): U66's case SS9 fails there and passes at the root. The page keeps `<base href="meguri-media://media/">`, so relative and root-relative media and fonts resolve at the media root (R493, R501). U67 either serves the document at `/` or loads it with `loadHTMLString(_:baseURL:)` and that base; U66's test harness does the first. `/` never names a media file. Only the bundled runtime assets are under the reserved `/_anki/` prefix (R496).
- Every response carries `Accept-Ranges: bytes`; a single `Range` gets 206 with `Content-Range: bytes first-last/total` and exactly those bytes (R495). WebKit on both platforms sends small probes (`0-1`, `0-7`), a 64 KB head, a 3-byte tail (ID3v1) and 64 KB windows at the seek target.
- Module scripts and fonts get `Access-Control-Allow-Origin: *` (R495).
- SVG and HTML media files get `Content-Security-Policy: script-src 'none'`; the page document at `/` and the runtime assets under `/_anki/` get none (R497, R504).
- Storage: one `WKWebsiteDataStore(forIdentifier:)` with a fixed identifier for every card surface (R502). The spike used its own identifier (`6D656775-7269-5350-494B-453230303032`), never the production one U67 chooses. The write route waited 3 s before the relaunch; whether localStorage written just before a kill survives is not established.
- Open: S2(a) `<audio>` and `<video>` on an iOS 27 device. The plan does not defer it to U256, so it stays open here until it runs, or until the plan defers it.

## Runbook: the iOS device check (Antoine)

1. An iPhone on iOS 27 with Developer Mode on (user step `device-developer-mode`), unlocked, attached to this Mac.
2. Run `tools/mg ui-test --list-physical-devices` and select the connected iPhone identifier. Run the command below. It uses the Profile scheme, Performance plan and development bundle, playing the silent MP3 and muted video.
3. The audio test passes when the MP3 seeks to at least 470 s of 525.5 s and plays on; the video test, when the video seeks to at least 17.9 s of 20 s and plays on. Each needs ranged requests in the last fifth, every one answered 206 with the `Content-Range` asked for and the file's bytes. Their activities `S2(a) audio {...}` and `S2(a) {...}` hold the numbers; record them in the results table above (a device column), with the device model and iOS build. Both passing confirms the configuration on iOS; either failing is S2(a) failing (Decision).


```sh
tools/mg ui-test --physical-device <identifier> --config Profile \
  --suite CardSwapTimingSpikeTests/testAudioSeeksToNinetyPercentThroughRangeRequests \
  --suite CardSwapTimingSpikeTests/testVideoSeeksToNinetyPercentThroughRangeRequests
```

### Device attempt, 2026-10-20

Antoine authorizes both checks and connects an unlocked, paired iPhone with
Developer Mode enabled. Inventory `ui-test/20261020T061557Z-52023` reports an
available iPhone18,1 on iOS 27.0.1 (24A446). Run
`ui-test/20261020T062002Z-55370` stops during provisioning: Apple reports
“PLA Update available” and requires acceptance of the updated Developer Program
License Agreement. No app was installed and neither test ran. Both results remain
pending; this build failure does not establish an S2(a) media failure.


Antoine accepts the agreement. Run `ui-test/20261020T062310Z-67240` then
creates profiles but stops on the app and widget App Group entitlement mismatch.
Run `ui-test/20261020T062605Z-69473` has the same result with
`REGISTER_APP_GROUPS=YES`; that setting is defined in Xcode's macOS specifications
and is removed from the physical iOS command. Neither attempt installs the app
or runs a test. Pending setup: register `group.com.antoinefink.meguri.dev` in
the developer account and assign it to both development App IDs. Keep the
existing entitlements. The next run must refresh profiles and execute both tests.
