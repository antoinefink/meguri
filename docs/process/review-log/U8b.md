# U8b review log

Unit: U8b, Mac UI suites in a macOS VM: `mac-ui`, `a11y --platform mac` and `drive --platform mac` build on the host and run their XCUITest suites in the test VM (`docs/process/mac-ui-vm.md`), unthrottled; without the VM they are queued (user step `mac-vm`).
Review rounds run: 2. No spec branch: the unit changes only `tools/` and docs. Implementer session s-20261015T101826Z-7f1f1446 built the unit (c565c47). The round entries below come from the fix commits.

## Round 1 (fixer s-20261015T110415Z-9ab0b885; fixed in 5d696a6)

Two findings in the VM runner (`tools/mg.d/mac-vm.sh`).

- A passing run could report pass with no result. When xcodebuild exited 0 in the VM but the SSH call that looks for the result bundle failed, the copy was skipped and nothing was recorded. The probe now tells a missing bundle from a failed call. A failed call, a passing run without a bundle, or a copy that brings none back is `mac-vm-copy` (error).
- A run that lost its SSH session went on in the VM, and the next suite cleared the flag the exit trap relied on, so two XCUITest runs could share the VM's one login session. The job now stops the lost run at once and checks that no `xcodebuild test-without-building` or UI-test runner is left. If it cannot, mac-ui runs no more suites (`mac-vm-stop`). Every job also stops a leftover run when it takes the VM, before copying or testing, which covers a job that was killed outright.

The bats fakes now model the VM's processes (a run that starts beside another logs `vm-overlap`). Each defect has a test in `tools/tests/mac_vm.bats`.

## Round 2 (no fix commits)

No finding needed a fix.

## Open after round 2

None.

## Integration (2026-10-15)

Integrator, in `meguri-wt/U8b` (unit/U8b); the similarity scan ran from the main checkout.

- No spec/U8b branch exists. feat/meguri-v1 (2df538e) was already the base of unit/U8b, so the merge did nothing.
- Similarity, `--rev unit/U8b --base feat/meguri-v1` (20261015T115331Z-56558): 0 files scanned (the 20 changed files are shell and Markdown, out of scope), 0 flagged.
- The host config `~/Library/Application Support/meguri/mac-vm.json` was already there (written during implementation). It names `/Volumes/T9/meguri-vm/bin/tart`, `/Volumes/T9/meguri-vm/bin/vm-ssh` and `meguri-mac`. The VM was running.
- Verified at 5d696a6, the parent of this log commit (feat/meguri-v1 has no `tools/mg gate` yet):
  - Rust and tools: core-test 488 passed (20261015T114129Z-22776); core-lint (20261015T114403Z-32240) and core-deny (20261015T114440Z-34270) clean; selftest 228 passed (20261015T114444Z-34568); selftest-host 24 passed (20261015T120347Z-79148).
  - Goldens and conformance: `goldens-check --base feat/meguri-v1` (20261015T114936Z-48251): 119 fresh, 0 changed. Conformance T0 (20261015T114959Z-48919): 6 live, 65 pass, 48 pending and 0 fail on each lane.
  - App: swift-test MeguriCore 15 passed (20261015T114129Z-22757) and MeguriKit 27 passed (20261015T114156Z-24279). build-app Debug passed for ios-sim (20261015T114231Z-25703) and mac (20261015T114257Z-27285). xcode-check (20261015T114321Z-29897): 110 passed, 1 skipped. The skip is the Mac TestHooksTests, which runs through `tools/mg mac-ui`.
  - doctor (20261015T115348Z-57044): `mac_vm` configured; meguri-mac on a connected drive, present, running, SSH up, Xcode 27.0 (27A266a), the same as the host's.
- Real runs in the VM, from this worktree, with the host unlocked. A watcher checked the host lease every 2 s; no run of this checkout took it. Another worktree (U11) held it for a simulator job during the runs, and the VM runs did not wait for it.
  - `mac-ui --suite a11y` twice in a row: passed both times, 58 s (20261015T115640Z-61284) and 52 s (20261015T115737Z-65581). `mac_runner: "vm"`, VM lease waited 0 s, no hourly refusal. `a11y.xcresult` is in the run directory.
  - `a11y --platform mac` (20261015T115829Z-69613): 11 passed, 1 skipped, `mac_runner: "vm"`, `mac.xcresult` in the run directory.
  - Full `mac-ui` (20261015T115928Z-72610), 259 s: agent-driver, a11y, mac-window-sizes and sample-routes passed. test-hooks, open-sample-sheet and sample-transition ended `blocked: mac-input`: the VM runs macOS 27.0.1, which drops XCUITest's synthesized input as on the host (user step `mac-input`).
  - A run with the host locked was not exercised here. `tools/tests/mac_vm.bats` covers it with a fake session.
- Provenance: `meguri-wt/U8b` was made by `tools/mg worktree new` (sparse `/*`, `!/oracle/`); no `oracle/` is on disk.

## Remaining hosted checks: VM migration (2026-10-19)

Start: `f06073394f2394777ef485bb393d9fbe0a43b6f0` on master. Acceptance requires a clean full unit gate from this start; M0 closure is evaluated separately.

Antoine asked for unattended runs with the host locked and reported occasional Meguri windows on the host. The U11 supplement already moved fixture argument launches to the VM. This supplement moves Mac snapshots, Xcode's hidden Debug launch probes and its Mac Unit/Snapshot plans there too. Builds stay on the host. Execution uses the existing VM lease, with no host fallback in these wrappers. Hidden-launch liveness, Dock and foreground checks remain intact.

Restricted fixer sessions `s-20261019T155858Z-92af408b` and `s-20261019T162312Z-ff77d373` produced source commits `32585412` and `7ca1cb31`. Both clean-room runs passed their boundary probes and removed their temporary authentication. No app, core, specification or baseline changed.

Reviewer session `s-20261019T161902Z-fe1aeb46` completed two rounds:

- Round 1 found three P2 groups: output transport could hide a completed snapshot failure; a typed Failed Xcode summary could pass TAP with exit zero; six older regression fixtures still assumed host execution.
- Round 2 found no P0–P2 issues after the fix. Snapshot failures become blocking before optional transport. Xcode requires exit zero, a Passed summary and zero failed tests. Migrated fixtures preserve phase numbering, lease waiting, independent build failures, diff artifacts, selective recording and unchanged passing reference bytes.

Verification so far:

- Original routing regressions: five RED failures before the routing patch (`selftest/20261019T161626Z-61174`). All five passed afterward; the broader first run retained six fixture failures (`selftest/20261019T161711Z-61779`), subsequently fixed.
- New failure-precedence regressions: seven RED failures before the second production patch (`selftest/20261019T163240Z-2422`). The focused run afterward passed 56/56, with no skips (`selftest/20261019T163257Z-2857`). These are working-tree checks, not clean-tip acceptance evidence.
- Host Bash 3.2: 28/28 passed on clean `7ca1cb31` (`selftest-host/20261019T163803Z-8799`).
- Full unfiltered VM Mac matrix (`snapshots/20261019T163420Z-3630`): its actual model key, `VirtualMac2,1-26A434`, had no references. All 60 comparisons were new; this did not prove image equivalence.
- Controlled comparison with the existing reference-key option, `TEST_RUNNER_MEGURI_SNAPSHOT_KEY=Mac15,9-26A434`, against the unchanged committed images (`snapshots/20261019T163541Z-6266`): 56 passed, four failed at the existing tolerances. The VM's renderer identity remains distinct from the reference key.

All four differences are in `dark-standard-macDefault`:

| Subject/state | Matching pixel fraction | Lowest perceptual precision |
|---|---:|---:|
| `designsystem.emptyState/typical` | 0.9999644 | 0.96458983 |
| `sample.deckOverview/empty` | 0.9999957 | 0.96458983 |
| `sample.deckOverview/typical` | 0.99997884 | 0.93789065 |
| `sample.deckOverview/overflow` | 0.99995846 | 0.93421876 |

Required fractions remain 1.0 and 0.98. Antoine approved VM-specific baselines in chat on 2026-10-19 after reviewing the four differences. The comparison is `build/orchestration/U8b-mac-vm-comparison.html`. Host references and comparison tolerances remain unchanged.

- Recording (`snapshots/20261019T171900Z-56838`): 60 references created only under `app/Snapshots/VirtualMac2,1-26A434/`; committed in `5a037342` with the chat approval recorded. Existing host reference files and the signature approval ledger are unchanged.
- Fresh default verification (`snapshots/20261019T171952Z-59085`) on clean `5a037342`: all 60 comparisons passed without record mode, a filter or a reference-key override. The returned test bundle had 63 passed, zero failed and one existing skip.
- Round 3 through `53c2b0ba`: no P0–P1 or material P2 findings. The reviewer checked the isolated 58 PNG and two HTML references, unchanged host reference set and tolerances, explicit approval, execution documents and fresh comparison receipts. This does not add signature-ledger approvals: its guard checked zero signature entries.
- The final tip carries `Unit: U8b`. Accept only after `tools/mg gate --base f06073394f2394777ef485bb393d9fbe0a43b6f0` passes on that clean tip; focused checks and the snapshot comparison alone are not acceptance evidence. M0 closure is not claimed by this supplement.

### First full gate and follow-up

The full gate on clean `bb9da8a0` (`gate/20261019T172300Z-62580`) was stopped after concrete failures returned. Its result is cancelled and incomplete, not an acceptance receipt. Bats passed 488/488, host Bash passed 28/28, both default app builds passed, hidden VM launch probes passed, and the Mac hosted Snapshot plan passed 62 tests with two existing skips. The iPhone hosted Snapshot plan passed 49 with two existing skips.

- AppStore build: the core configuration safety check found an incomplete shared framework stamp. A parallel Swift package job built Debug and replaced seven framework outputs during the AppStore build. This was an orchestration error; Apple jobs now run one at a time. No stamp check, configuration policy or cache rule was weakened.
- Mac Unit plan: 102 passed and two failed. The native MP3 probe timed out after 65.8 s on the VM's missing audio route; its following video navigation timed out after 60.8 s. The existing UI audio test and S2 decision already record this machine limit. The native audio probe lacked that hardware condition after migration.

Restricted fixer `s-20261019T174222Z-4a71f8e9` added the same fail-closed `kern.hv_vmm_present` condition to the native MP3 test in `baaa7a2d`. The test reports disabled before creating audio in a VM or when hardware cannot be established. The audio body, video enablement, all media assertions and all timeouts are unchanged. The clean-room run passed its boundary probes and removed authentication (`cleanroom/20261019T174256Z-18848`).

Focused hardware control on clean `baaa7a2d`: `swift-test --package MeguriKit -- --filter CustomSchemeMediaSpikeTests` passed 8/8 with zero skips (`20261019T174510Z-20111`). This proves the audio test still runs on confirmed hardware. The next full VM Unit run must prove video passes with only audio unavailable; no successful VM audio result is claimed. The first gate's logs and both failure bundles were preserved under `build/orchestration/U8b-gate1-evidence/` before the shared Xcode work directory is used again.
