# Spike S2: the custom-scheme media origin

Date: 2026-10-16. Unit: U65. Toolchain: Xcode 27.0 (27A266a). Hosts: this Mac (Mac15,9, M3 Max, macOS 27.0.1 26A434), and the test VM `meguri-mac` (macOS 27.0.1 26A434), whose pool simulator Meguri-Pool-iphone-1 is an iPhone 17 Pro on iOS 27.0 (24A434). Question (KTD11, swiftui-arch §1.3): can a `meguri-media:` scheme handler serve card media with Range, `fetch()`, module imports, fonts and persistent storage, and keep scripts in media documents from running (R493–R503, R560)? If S2(a), (b) or (d) failed, U66 and U67 would take the `loadFileURL` fallback.

## What was run

- `app/Packages/MeguriKit/Tests/CardRenderingTests/Spikes/CustomSchemeMediaSpikeTests.swift` on this Mac (`tools/mg swift-test --package MeguriKit`): WKWebView in an off-screen window, a throwaway handler (`Spikes/Support/SpikeMediaHandler.swift`) that serves in-memory files at the root of the origin, answers one `Range` with 206 and logs every request. Media is made at run time: a silent constant-bitrate MP3 (MPEG-1 Layer III, 128 kbit/s, 8,388,789 bytes, 525.5 s), an H.264 MP4 (20 s, fast start), and system faces rebuilt as standalone sfnt files.
- `app/MeguriUITests/CardSwapTimingSpikeTests.swift` (`testVideoSeeksToNinetyPercentThroughRangeRequests`, `testStorageOnTheMediaOriginSurvivesARelaunch`) through `tools/mg perf-ui` in the VM, against the test-hook routes `spike/media-seek` and `spike/storage/{write,read}` (`app/Meguri/TestHooks/MediaOriginSpike.swift`).

## Results

| Item | macOS host | iPhone 17 Pro simulator (VM) |
|---|---|---|
| (a) `<audio>` 8 MB MP3 seek to 90 % and play | pass: seeked to 472.95 s of 525.5 s, played to 473.85 s; ranged requests `0-1`, `0-7`, `0-65535`, `8388661-8388663`, `7549368-7614903`, `7614904-7680439`, each answered 206 with `Content-Range: bytes a-b/8388789` and that many bytes | not checkable: metadata loads through ranged 206 answers, then `play()` never settles and the seek never completes. The same bytes from a blob URL stall the same way: the VM's simulator has no audio route. These were exploratory runs; the kept test checks video only, because an audio element left loading in the VM once stalled the app's main thread for a minute. Device checkpoint (U256). |
| (a) `<video muted>` H.264 seek to 90 % and play | pass: about 70 ranged requests, all 206 | pass: seeked to 18.0 s of 20 s, played to 18.42 s; 97 ranged requests answered 206, 13 of them in the last fifth |
| (b) `import('./_lib.js')` and `fetch('_data.json')` from the card page | pass (`text/javascript`, `application/json`, `Access-Control-Allow-Origin: *`) | not run separately |
| (c) `url("/_root.ttf")`, `url("_relative.ttf")`; `plain.png`, `./dotted.png`, `/rooted.png` from a page under `/_anki/` with `<base href="meguri-media://media/">` | pass: every URL requested at the root, both faces loaded, all images decoded | not run separately |
| (d) localStorage and IndexedDB in `WKWebsiteDataStore(forIdentifier:)` | pass: a later page with a new store object for the same identifier reads both values; another identifier reads neither | pass across an app relaunch: written, app terminated, read back equal from both |
| (e) script in SVG and HTML media opened as documents | pass with `Content-Security-Policy: script-src 'none'` on those responses: no script ran in an SVG iframe, an HTML iframe or a top-level SVG; `<script src="_x.js">` in the card ran | not run separately |
| (f) origin forms | `meguri-media://media/` (origin `meguri-media://media`), `meguri-media:///` (origin `meguri-media://`) and `meguri-media://Media.Local/` (origin `meguri-media://media.local`) all fetched, imported and kept localStorage and IndexedDB | `meguri-media://media/` used for (a) and (d) |

In the macOS VM the same app checks passed: the video seek (112 ranged answers, 20 in the last fifth) and storage across a relaunch. The video check passed on the iPhone simulator in two runs (97 and 99 ranged answers).

Red runs before the handler did Range and CSP (same tests, handler with both off): the MP3 and the MP4 still played, because WebKit read whole files answered with 200 to ranged requests, but no ranged answer existed, so (a) failed; and the SVG and HTML scripts ran, so (e) failed.

## Decision

- No fallback: S2(a), (b) and (d) passed. U67 serves card pages and media from a `WKURLSchemeHandler` on `meguri-media:`; `loadFileURL(_:allowingReadAccessTo:)` is not needed.
- Origin form: `meguri-media://media/`. A fixed lower-case host gives a tuple origin (`meguri-media://media`); the empty-host form also worked on 27.0, but its origin (`meguri-media://`) is an edge of the URL standard, and host case folds (`Media.Local` became `media.local`). The host never changes (R493).
- Pages live under the reserved `/_anki/` prefix with `<base href="meguri-media://media/">`, so relative and root-relative media and fonts resolve at the media root (R493, R496, R501). A page loaded with `loadHTMLString(_:baseURL:)` at the origin root would need no base element; U67 picks either, the S2 tests cover the first.
- Every response carries `Accept-Ranges: bytes`; a single `Range` gets 206 with `Content-Range: bytes first-last/total` and exactly those bytes (R495). WebKit on both platforms sends small probes (`0-1`, `0-7`), a 64 KB head, a 3-byte tail (ID3v1) and 64 KB windows at the seek target.
- Module scripts and fonts get `Access-Control-Allow-Origin: *` (R495).
- SVG and HTML media outside `/_anki/` get `Content-Security-Policy: script-src 'none'`; runtime pages under `/_anki/` get none (R497, R504).
- Storage: one `WKWebsiteDataStore(forIdentifier:)` with a fixed identifier for every card surface (R502). The spike used its own identifier (`6D656775-7269-5350-494B-453230303032`), never the production one U67 chooses. The write route waited 3 s before the relaunch; whether localStorage written just before a kill survives is not established.
- Open for the U256 device checkpoints: S2(a) audio seek on an iOS 27 device, which the VM's simulator cannot play.
