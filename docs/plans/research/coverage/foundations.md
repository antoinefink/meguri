# Coverage note: foundations


Coverage map (unit → R-IDs):
- U1: R1968, R1969, R1952 (placement baseline).
- U2: R1954, R1955, R1956, R1957, R1958, R1959, R1960, R1987 (role rule), R2021 (source tree), R1881 (process); AE85.
- U3: R1868, R1869, R1870, R1871, R1872, R1873, R1874 (bump mechanics), R1876 (container network), R1882 (bump-time harvest hook), R1885 (oracle metadata).
- U4: R1874 (regeneration/diff), R1877, R1878, R1879, R1880, R1881, R1882, R1883, R1884, R1885, R1886, R1887, R1888, R1889, R1950 (T0/T1 definition); AE83 (mechanism).
- U5: R1875, R2025 (Rust), R2027, R2021 (dependency graph).
- U6: bridge mechanics supporting R1857–R1861 (those requirements are outside my assigned ranges; owned as conventions by every UI/core unit).
- U7: R1963, R1964 (convention), R1967, R2031 (configuration), R2041 (entitlements).
- U8: R1961, R1962, R1964 (audit), R1965, R1966, R1968, R1969, R1976 (capture and contact sheets).
- U9: R1979, R1986, R1988 (decision log), R1992, R1993, R1994, R1995, R1996, R1997 (brief/rubric), R1998, R1999, R2001; AE90 (lint part).
- U10: R1970, R1971, R1972, R1973, R1974, R1975 (tooling), R1977, R1978, R1980, R1983, R1984, R1985, R1986, R1987, R1988, R1989, R1990, R1991 (status flow); AE90 (critic part).
- U11: R1804, R1805.
- U12: R1861, R1862 (contract), R1863 (harness), R1864, R1866, R1867.
- U13: R1876; infra for R1926, R1932, R1933.
- U14: KTD24; R1886, R1889 (fp-libm-boundary deviation class).
- U15: R1806 (fixture side), R1915, R1938, R1939, R1940, R1941 (R1938–R1941 are outside my listed ranges but are the "real-collection fixture handling" infrastructure; orchestrator please confirm ownership).
- U16: R2022, R2024, R2025 (Swift/shipped scope), R2026, R2023 (wording in template).
- U17: R1869 (bundle check), R1967 (Release check), R2021 (shipped targets), R2023, R2028, R2029, R2030 (check), R2031, R2032, R2033, R2034, R2035, R2036, R2037, R2038 (check), R2039, R2040 (check), R2041 (check), R2042; AE89.
- U18: R1806 (must-beat), R1865.
- U19: R1950, R1951, R1952, R1953, R1991, R1880 (release gate), R1863/R1937 wiring; AE82 (ordering).

R-IDs in my ranges owned elsewhere:
- R1981, R1982 (accessible alternatives to tap zones; rating color never the only cue) → Review UI (U175); U10 provides the audit/VoiceOver harness; AE88 suite owned by U175.
- R2000 (chrome never restyles card content) → Rendering (U66) and Review UI (U175); stated in the U9 brief/rubric.
- R2030 honest client identity → Sync (U105, with R1509); U17 only checks the binary.
- R2038 card-script bridge allow-list → Rendering (U66/U67); U17 checks against the published allow-list.
- R2040 web inspection Debug-only → Rendering (U67); U17 checks.
- R2029 About-screen wording and R2022/R2024 Acknowledgements screen UI → Settings (U160); U16 supplies `Acknowledgements.json`, U17 checks store text.
- R1867 surfacing diagnostics in Copy Debug Info → Settings (U160).
- R1863 per-interval suites, R1942, R1943 private-suite contents, R1934–R1937 differential fuzzer, R2003 fault injection, critic runs per screen → Perf/QA (U240+); my units provide tooling and tier wiring.
- R1890–R1949 conformance suites → the respective areas (sched, rendering, search, stats, packages, sync, servers); my units provide the format, runner, oracle and rigs.
- R2042 foldable screenshots and actual submission → release unit (U240+); U17 checklist.
- R1975 DOM assertions for card faces → Rendering (U66/U67, R1912).

Cross-area contracts assumed:
1. Clock and randomness seam (U5, in meguri-model): every core crate takes time and randomness only through it; id allocation (U23), day clock (U40), fuzz (U45), load balancer (U44) and the engine (U24) must use it. Lint enforces no direct system clock reads.
2. Executor adapters: each core area adds op/observation adapters in `core/crates/meguri-testkit/src/exec/<area>.rs`, registers capabilities, and marks its scenarios active in `conformance/scenarios/<area>/area.yaml`. Driver ops per area live in `oracle/driver/ops/<area>.py` (oracle-maintainer role).
3. Engine `test-hooks` exports (clock override, open fixture) for U7's launch options are implemented by U24; UI units register their routes in `app/Meguri/TestHooks/RouteRegistry.swift` and conform screens to `PreviewStates` (U10).
4. Visible-confirmation message from card runtime (U66) to CardRendering (U67) ends the R1862 interval through `Instrumentation/VisibleConfirmation.swift` (U12); interval names come only from `IntervalNames.swift`.
5. Glass only via `DesignSystem/Chrome/` or the lint allow-list (U9); reusable components go into the gallery first.
6. U11 and U15 write/read collections through meguri-model (U21), storage open/create (U22) and row I/O (U23); they need bulk-insert paths from U23.
7. Widget target (U165) ships the privacy manifest created in U17; the widget never links the Rust core.
8. Add-on oracles (Review Heatmap for AE58/U95; Anki desktop + AnkiConnect recordings for R1945/U141) use the U3 extension mechanism as separate quarantined images, requested by those areas.
9. Rendering units publish the card-bridge message allow-list file that U17 checks (R2038).
10. Output Structure additions: `conformance/schema/`, `conformance/deviations.yaml`, `conformance/goldens-apple/`, `conformance/fixtures/`, `conformance/recordings/`, `conformance/private/`, `core/tools/uniffi-bindgen-swift/`, `core/tools/layering-check/`, `core/pins.toml`, `core/bench/baselines/`, `oracle/macos/`, `oracle/harvest/`, `oracle/provenance/`, `tools/similarity/`, `tools/gate/`, `tools/release-audit/`, `tools/notices/`, `docs/distribution/`, `app/Config/`, `app/TestPlans/`, `app/Meguri/TestHooks/`.

Conflicts and open questions:
- Resolved in KTD9: binaryTarget `MeguriCoreFFI` (C module), Swift 5 target `MeguriCoreBindings` (generated Swift), Swift 6 target `CoreBridge`.
- R1877's fixed counts (726 Rust, 247 Python) are wrong at tag 26.09.3 (673 Rust, 225 Python, 34 TS); U4 counts at the tag and records counts in the coverage report.
- R1885 "wheel hash" is implemented as the lock-file hash (covers both per-arch wheels).
- R1890/R1892/R1902 exactness on Apple vs the Linux oracle (KTD24): U14 introduces Apple overlay goldens and an R1889 deviation class `fp-libm-boundary`; if Meguri-macOS diverges from Anki-on-macOS, a new unit (patched fsrs with vendored math) is needed.
- KTD2 says rand at "Anki's version": at tag 26.09.3 rslib's fuzz uses rand 0.9.4 (fsrs 6.6.2 brings 0.10.1). U5 pins 0.9.4 for fuzz; rust-stack.md's "0.10.x" is superseded by oracle-harness finding 2.
- R1866 "Release configuration" vs R1967 "no test hooks in Release": U7 adds a Profile configuration (Release optimization + hooks, never distributed) for performance tests. Confirm this interpretation.
- R1957 "every release since 2.1.50": the local Anki clone lacks the 2.1.50 tag; U2 fetches it once.
- R2001 / contract `docs/design/` reference screenshots of Apple and third-party apps: fine while the repo is private; if the repo becomes public they should move out of git. Needs a user decision.
- Signing: until the user adds the Xcode account, Mac builds are ad-hoc (no App Group, so widget and App Group behavior on Mac unverified), device builds and device performance (R1863 devices, T4) are blocked.
- R1962/R1971: no foldable simulator in Xcode 27.0 (approximated by sizes); resizable iPhone windows via `devicectl appResize` and Liquid Glass look/opacity via `devicectl` appearance flags on simulators are unproven (spike S5 in U10); axes that cannot run on simulators move to the monthly device session.
- R1978: VoiceOver service support on simulators is unverified; U10 has a fallback on accessibility-element order.
- R2042 export compliance: TLS via rustls/aws-lc-rs (non-Apple crypto, HTTPS only) — the exemption answer needs the user's confirmation.
- The macOS-native oracle lane (U14, U18) is an explicit exception to Docker-first; time control there (libfaketime on macOS) is unproven, with a "time by data" fallback.
- No git remote exists, so "CI" means local `tools/mg gate` plus a user-approved scheduler entry for nightly runs; the R1937 nightly budget depends on that entry.
- Contract `docker/compose.yml` lists a `syncserver` service while KTD23 runs the server as a child of the oracle worker; U13 keeps both (child server for conformance, compose service for app end-to-end runs).
- R1863 says simulator performance tests run "for every change"; U19 runs a smoke subset per change touching app/core hot paths and the full set per milestone. Confirm.
