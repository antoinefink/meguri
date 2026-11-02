# Cloud batch integration gate, 2026-11-02

Status: failed; no batch, unit or milestone acceptance.
Original batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Gate: gate/20261102T075235Z-35258, clean source 60b8cc90.
Source stayed unchanged until the original gate process exited.

MG reports 47 runs: 33 in its passed category, 5 failed and 9 skipped.
A warning in the passed category is not acceptance evidence. Failed run IDs:
core-test, swift-test-kit, xcode-check, snapshots and mac-ui. T1 did not run.
The full gate is not to be repeated until known technical issues are addressed
with focused checks. All original results and failed assertions remain retained.

- Rust: 4,387 of 4,388 executed checks passed. The single failure measures
  suspend's deadline from before thread creation until after join. A prepared
  test-only correction measures the actual worker call, retaining the same
  20 ms timeout, exact error and 200 ms assertion. Independent review is clear.
- Core Swift: swift-test/20261102T085734Z-55243 passed 367/367, no skips.
- Kit Swift: swift-test/20261102T090842Z-60711 recorded the existing R554
  inter-clip audio assertion, then stopped making progress. After a bounded
  stack-only sample and confirmation of the owned process chain, the original
  MG process was terminated. This run is incomplete, not a passing tally.
  Neither the last buffered test name nor its total duration proves the cause.
- Xcode Mac Unit: 977 passes, 33 failures and 4 skips. Thirty-two failures
  lacked required real fixture/helper inputs. Their delivery repair has a
  separate log. The remaining NoCollection unavailable failure needs a
  fixed-category diagnostic; the current result hides the underlying branch.
- App-hosted Mac: 85 passes, 5 failures, 2 skips. App-hosted iPhone: 67 passes,
  3 failures, 2 skips. The cloud soak entry stopped at its dedicated-directory
  guard; no SDK execution occurred. Snapshot precision differences, Mac
  settings search, and the frozen manual-transfer Keychain case remain recorded.
- Snapshots: 2,170 reported cells, 513 passes, 321 failures and 1,336 skips.
  No baseline or tolerance changes are justified by these counts. A current
  27.2 xctestrun contains the soak skip, but original consumed-file provenance
  is insufficient to prove why the soak still ran. Do not repair selectors
  on an assumed old configuration file.
- Mac UI: mac-ui/20261102T110748Z-72069, drive-open-sample-sheet, exited139
  in xcodebuild test-without-building with no completed summary. It is an
  execution error, not queued, optional VoiceOver or frozen server coverage.
  The smallest next diagnostic is the owned VM crash's fixed process, signal
  and faulting-library fields. No broad screen or diagnostic export is needed.

Static checks, lint, T0 conformance, goldens, native durability, tooling tests,
privacy/similarity checks and app builds supplied passing partial evidence.
Their individual receipts define coverage; none converts this full gate to pass.

After this gate, U114's two approved extra repairs and the generated-base and
VM-input components were saved as separately reviewed WIP. Native verification
remains required. No Unit trailer was added. R554 retains only its previously
approved interim scope; its enabled limits and final closure requirement stay.
