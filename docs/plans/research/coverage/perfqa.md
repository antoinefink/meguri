# Coverage note: perfqa


R-ID coverage map (unit → requirements)

- U240: Success Criteria (milestone structure); milestone cadence for R1863, R1943, R1950.
- U241: R1802, R1803, R1861, R1862 (gate definition), R1863 (rules), R1864 (thresholds), R1866.
- U242: R1803 (size matrix), R1804, R1805, R1823, R1842, R1843, R1846, R1847, R1848, R1849, R1850, R1855, R1864 (benchmarks).
- U243: R1807–R1817, R1862 (enforced on review), AE81 (simulator form), timing of AE12 and AE14.
- U244: R1818–R1822, R1824–R1839, R1841.
- U245: R1840, R1851–R1854, R1856–R1860, R1867.
- U246: aggregate gating for R1890–R1933 and R1944–R1949, R1889 enforcement, KTD24 native-oracle check, AE82, AE84 (aggregate).
- U247: R1865, R1918, R1934–R1937, nightly execution of R1901.
- U248: R1844, R1845, soak aggregation of R1926–R1933, AE9 and AE27 end to end.
- U249: R1806, R1938–R1943, private execution of R1902, R1906, R1910 and R1947 (AE87).
- U250: R2002, R2003 (core), R2004–R2006, R2007 (core), R2008, R2009, R2011, R2012 (core), R2013, R2014, R2015 (core), R2016, R2017, R2019, AE86, AE83 (preservation side).
- U251: R2003 (app), R2007, R2010, R2012 (UI), R2015 (UI), R2018, R2020, AE86 (app path).
- U252: R1970–R1976.
- U253: R1977–R1982, AE88.
- U254: R1983–R1991, AE90 (critic part).
- U255: CI checks for R2032, R2035, R2042 and AE89; verification of R1967, R2028, R2030, R2034, R2036, R2037, R2040, R2041.
- U256: device legs of R1802 and R1863, R1990 support, AE81 (device), the R1913 reference capture step.
- U257: Success Criteria (release gate), R1880 (release rule), R1943 (release run).

R-IDs in my assigned groups whose scenarios or behavior are owned elsewhere (my units only aggregate or run them)

- R1890–R1897 (scheduler and queue scenarios): Scheduling area (U41, U45, U44). R1892 and AE82 use the fuzz table from U45 and U4.
- R1898–R1901 (rollover and TZ): Scheduling area (U40). U247 runs R1901 nightly.
- R1902–R1906 (FSRS conformance): Scheduling area (U45, U46, U47). U249 executes R1902 and R1906 on the private collection.
- R1907–R1915 (rendering conformance, including the web-view corpus R1912 and the reference-screenshot comparison R1913): Templates and rendering area (U60, U61, U64, U66, U67). U256 lists the one-time manual reference capture as a user step.
- R1916–R1920 (search and sort): Search and browser core area (U80, U84). R1918 (grammar fuzzer) is owned here in U247 because it is fuzz infrastructure. Move it if the search area prefers to own the generator.
- R1921 (stats conformance): Stats area (U95, U98).
- R1922–R1925 (packages and Check DB): Packages and maintenance area (U120, U122, U124, U128).
- R1926–R1933 (sync conformance): Sync area (U105–U111). U248 adds the multi-party soak on top.
- R1944–R1949 (local server contracts): Mac servers area (U135, U137, U141). U249 runs R1947's replay because its payloads come from private data.
- R2002–R2020 behavior: storage (U22, U23), engine (U24, U25), derived store (U26), media (U31), sync (U107, U109, U111), packages, backups and maintenance (U120–U130), app shell and lifecycle (U151, U160). My units own only the suites that prove them.
- R1868–R1889 (oracle, harvest, scenario format), R1950–R1969 (tiers, clean room, verify loop) and R1992–R2001 (design guardrails) are Foundations (U2–U13, U9). My units consume them.
- R2021–R2042 are outside my assigned groups. U255 owns only the CI checks named above. Implementation owners need assigning by the orchestrator: probably Foundations U5 for R2021–R2027, the app-shell area U160 (About and Acknowledgements) for R2026 and R2029, the sync area U105 for R2030, U7 for entitlements and manifests in R2034 and R2041, and rendering U66 and U67 for R2038–R2040.

Cross-area contracts assumed (orchestrator to reconcile)

1. **Signpost name registry (U12).** One file lists every R1861 interval name. Feature units emit exactly those names. U241 enforces completeness. The review unit (U175, U67) must emit `review.answerToNextVisible`, ending on the web content's post-frame paint confirmation (R1862), and must expose the frame probe hooks U243 needs. Spike S1 must validate this attribution.
2. **Fault points (U250 → core units).** A `faultpoint` facility in `meguri-model` behind feature `test-hooks`. U22–U31, U107, U109, U111 and U120–U130 must place named points at transaction and commit, temp-write, verify, rename and sync-stage boundaries. Each long op must also write an "acknowledged" journal entry in test-hook builds.
3. **Injectable free-space provider in the core.** Used by import, sync, backup, export and media download for R2015. A test hook overrides it.
4. **Cancellable-op registry in meguri-engine (U24).** Lists every long op, so U245's cancellation suite enumerates them automatically.
5. **CoreBridge debug call counters per signpost name (U6),** used for R1859. The debug main-thread assertion (U6) is required by U245.
6. **Oracle driver `timed` op (U3, oracle-maintainer role)** for R1865 and R1806. Native-oracle host venv bootstrap for KTD24, under `tools/scripts/native-oracle/` (spec side).
7. **Scenario metadata tags** declaring the matrix dimension values each scenario exercises, written by every area's spec writer. U246's checker reads them.
8. **`docs/design/screen-inventory.yaml`** as a machine-readable inventory (R2001 owner: U9 or U10) with screen id, signature flag and states. U252, U253 and U254 consume it.
9. **Test-hook endpoints in the app (U7):**
   - revlog commit timestamp per card (R1817);
   - lock-held report after `prepareForSuspend` (R2007);
   - integrity report;
   - "operation in progress" signal for host-driven kills;
   - corruption flag injection;
   - low-disk override;
   - Reduce Motion, Reduce Transparency and glass-look overrides for the matrix.
10. **meguri-cli subcommands** added by my units: `bench` (suites), `fuzz-diff`, `fault`, `private-suite`, `anonymize`. U4, U11 and U12 own the CLI skeleton.
11. **Output Structure additions to confirm:**
    - `tools/perf/` (budgets, baselines, ratio history);
    - `tools/scripts/gates/`;
    - `app/TestPlans/` (xctestplan files);
    - `conformance/catalog/requirements-matrix.yaml`;
    - `conformance/reports/private/`;
    - `docs/process/milestones.yaml`, `docs/process/milestone-records/`, `docs/process/device-runs.md`, `docs/process/daily-driver-log.md`;
    - `docs/design/screen-status.yaml` and `docs/design/critic-calibration/`;
    - `core/crates/meguri-testkit/profiles/`;
    - gitignored `build/reports/` for job outputs.

Conflicts and open questions

1. **R1863 and R1951 vs simulator timing.** Absolute ms budgets cannot be enforced reliably on simulators. U241 enforces absolute budgets on devices and on the Mac core benchmarks. Per change on simulators, it enforces only frame-relative, hitch-ratio and memory budgets, plus the 10% regression rule on ms metrics. Confirm this reading of "on simulators for every change … fail when a budget is exceeded".
2. **R1807 measurement.** "Earliest display frame … against a baseline native view swap" needs a frame-attribution method that spike S1 must validate. U243 runs report-only until then.
3. **R1806 and R1865 "same Mac".** The oracle runs in a Linux VM under Docker, while Meguri runs natively. The nightly trend uses Docker. The release "must beat Anki" check uses the macOS-native oracle venv (the KTD24 exception), so the comparison is like for like on macOS. This extends the KTD24 exception beyond FSRS checks.
4. **Signature screens.** R1972 lists review, Home and deck list, deck page, stats, browser and editor. The Success Criterion lists home, review, browser, deck options and stats. U254 uses the union, which adds deck options to the R1972 full-matrix set. Confirm.
5. **Foldable cells** (R1962, R1971 foldable axes, R9, R10): no simulator in Xcode 27.0. They are carried as `deferred` in the milestone manifest and do not block M5 until the 27.1 SDK ships. Confirm that the release gate may pass with them deferred.
6. **R1978 VoiceOver on simulator.** `XCUIVoiceOverService` simulator support is unverified. The fallback is hierarchy-order assertions per change, with real VoiceOver in device runs.
7. **R1971 axes for glass look, transparency slider and Reduce Motion/Transparency** depend on spike S5 (devicectl appearance on simulators) or on in-app overrides. If neither works for glass looks, those cells run only in device sessions.
8. **The brief's "private collection suite R1940–R1944"**: R1944 is the AnkiConnect contract and belongs to the Mac servers area. I treated the private suite as R1938–R1943 (plus R1806 and the private execution of R1902, R1906, R1910 and R1947).
9. **R1877's harvest counts are off** (oracle-harness finding 5). The U246 coverage report records counts at the pinned tag, not the fixed numbers. Foundations (U4) should own the wording fix.
10. **R1803 at 500k.** The 2× loosening applies to time budgets only. The R1851 memory budget explicitly also applies at 500k, and U245 enforces 150 MB at both sizes.
11. **Deck options and Card info milestones.** I placed U232 (deck options UI) in M3 with automatic optimization, because M1 relies on desktop Anki for maintenance. Card info (R1841) is assumed to land with the browser in M2. The review ⋯ "Edit" action arrives with U190 in M2, so the M1 review menu lacks Edit. Confirm with the review and notes areas.
