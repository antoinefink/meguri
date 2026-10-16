# CardRenderingTests

- These run on the macOS host in a borderless window at (-30000, -30000) that is never ordered in (root AGENTS.md: nothing on Antoine's screen). WebKit runs no `requestAnimationFrame` there, and no frame is presented: anything that needs frames, the swap's frame, the keyboard or a relaunch goes into the app (`MeguriUITests/CardSwapTimingSpikeTests`, `TestHooks/SpikeRoutes.swift`) and runs through `tools/mg perf-ui` in the test VM.
- Suites that share WebKit and this process's memory nest in `RenderingSpikes` (`.serialized`, 2-minute limit): `swift test` runs every other test target in the same process, in parallel.
- Media is made at run time (`Spikes/Support/SpikeAssets.swift`: silent MP3, noise PNG, H.264 MP4, system faces rebuilt from their tables). Commit no binary fixture, and never play audible sound on the host.
- `WKWebsiteDataStore(forIdentifier:)` writes under `~/Library/WebKit/`: use a fresh identifier per test and remove it afterwards, also on failure.
- `Spikes/Support/` is U65's throwaway harness (`SpikeMediaHandler`, `SpikePagePair`). When U67's `MediaSchemeHandler` and `CardPagePair` land, point the spike tests at them and delete the support (decision records `docs/design/decisions/s1-*.md`, `s2-*.md`).
