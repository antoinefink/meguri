# U8c review log

Unit: U8c, simulators in the macOS test VM. The simulator pool (`tools/scripts/sim-pool.sh`, `tools/mg sim`) lives in the VM of U8b. Every simulator job (`sim-setup`, `sim-reset`, `run`, `drive`, `ui-tree`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`) builds on the host, takes the VM's lease instead of the host lease, runs `simctl`, `devicectl` and `xcodebuild test-without-building` in the VM, and copies its artifacts back. Without the VM these jobs are queued (user step `mac-vm`); `MEGURI_SIM_ALLOW_HOST=1` keeps the host pool. `tools/vm/add-sim-runtime.sh` adds the pinned iOS runtime to the VM. The VM, its provisioning and the unit's real runs are recorded in `docs/process/mac-ui-vm.md`.
Review rounds run: 3. No spec branch: the unit changes only `tools/` and docs.

## Sessions

- Implementer s-20261015T120856Z-ccf8bde4: e6df25a. It provisioned the VM's iOS 27.0 (24A434) runtime and the pool, and ran the real runs recorded in `docs/process/mac-ui-vm.md`.
- Fixer s-20261015T144300Z-a88095e7 (round 1): 86c9e5d.
- Fixer s-20261015T154510Z-f9bb70e8 (round 2): 9e7caef (applied through Codex).

## Round 1 (fixer s-20261015T144300Z-a88095e7; fixed in 86c9e5d)

Four gaps in how jobs use the VM.

- `xcode-check` copied only its build products into the VM. Its tests read the snapshot baselines, the test-hook fixtures and the example drive scripts at their repository paths, and in the VM these were missing or stale. The script now copies them (`VM_INPUTS`) before its first VM run.
- `run` passed `-MeguriFixture` paths unchanged, and `drive` copied only `launch.fixture`. So the app opened its missing-fixture screen and the job passed. Every launch fixture now goes into the VM (`mg_vm_fixture`). A fixture that does not exist is an error before anything launches.
- `sim acquire`, `sim gc`, `sim-setup` and scripts that call `sim-pool.sh` created and booted simulators while another job held the VM. They now run under the VM's lease. A caller that already runs under the lease's holder does not wait for it again (`mvl_lease_held`). Release takes no lease.
- `xcode-check` left a run whose SSH session was lost still going in the VM, and started its next run beside it. It now stops that run (`mvl_stop_tests`) and starts no further run when it cannot.

## Round 2 (fixer s-20261015T154510Z-f9bb70e8; fixed in 9e7caef)

- `xcode-check --no-sims` still ran simulator tests on the host when `--iphone` or `--ipad` was given, without `MEGURI_SIM_ALLOW_HOST=1`. Simulator runs now start only when the VM runs them.
- `mvl_stop_tests` matched only the Mac runner's layout. An iOS runner that outlived xcodebuild was left running, and the next suite started beside it. The pattern now matches both layouts.

## Round 3 (no fix commits)

No blocking finding. One advisory finding is open (below).

## Open after round 3

1. [P2, advisory] The host selftest fails while the workstation session is locked (`tools/tests/mg_host_selftest.sh:778`). `tools/mg selftest-host` fails test 23, "host lease gate and sim setup on macos bash", with "second lease exit: got '2', want '0'". Both lease probes return blocked-host with `host_session: locked`, so the test exits before it checks `sim-setup` and recording. Fix: stub the session state in these sandbox tests, and test the real locked-session refusal in a separate test. The test passed in this integration pass (25 of 25), because the session was unlocked. (codex lens)

## Integration (2026-10-15)

Integrator, in `meguri-wt/U8c` (unit/U8c). The similarity scan ran from the main checkout.

- No spec/U8c branch exists. feat/meguri-v1 (883caff) was already merged into unit/U8c, so the merge did nothing.
- Similarity, `--rev unit/U8c --base feat/meguri-v1` (20261015T161607Z-17204): 0 files scanned and 0 flagged. The 31 changed files are shell scripts, bats tests, Markdown and the session ledger, which the scanner does not cover.
- Host config: `~/Library/Application Support/meguri/mac-vm.json` was already there (written for U8b). It names `/Volumes/T9/meguri-vm/bin/tart`, `/Volumes/T9/meguri-vm/bin/vm-ssh` and `meguri-mac`. The VM was running: macOS 27.0.1, Xcode 27.0, runtime iOS 27.0 (24A434) Ready.
- Verified at 9e7caef, the parent of this log commit. feat/meguri-v1 has no `tools/mg gate` yet.
  - Rust and tools: core-test 526 passed, 1 skipped (20261015T161549Z-16263). core-lint (20261015T161805Z-22400) and core-deny (20261015T161839Z-23505) clean. selftest 264 passed (20261015T161848Z-24074). selftest-host 25 passed (20261015T162703Z-48581), with the host unlocked.
  - Goldens and conformance: `goldens-check --base feat/meguri-v1` (20261015T161711Z-20526): 124 fresh, 0 changed. Conformance T0 (20261015T161744Z-21554): 6 live, 65 pass, 53 pending and 0 fail on each lane.
  - App: swift-test MeguriKit 27 passed (20261015T161621Z-17612). The first MeguriCore run (20261015T161554Z-16499) failed the timing test `progressFiredAThousandTimesASecondReachesSwiftAtMostElevenTimesInAnySecond`: Rust fired 734 reports a second, against 1,000 or more. core-test in Docker and U12's `bench-mac` loaded the host at that time. The test does not touch U8c's files. Rerun once the load was lower (20261015T162651Z-45086): 15 passed. build-app Debug passed for ios-sim (20261015T161642Z-18821) and mac (20261015T161652Z-19476).
  - doctor (20261015T161705Z-20137): `mac_vm` configured. meguri-mac is on a connected drive, present, running, and SSH answers. Its Xcode is 27.0 (27A266a), the same as the host's, and it has the simulator runtime.
  - xcode-check (20261015T163826Z-2490): 110 passed, 1 skipped, `sim_runner: "vm"`. This is the first real xcode-check through the VM: the implementer's pass had the host locked. Its simulator runs ran in the VM: iphone-app-hosted 42 passed; TestHooksTests on the iPhone (Debug, Profile, Release, AppStore) and the iPad; iphone DriveScriptTests 4 passed. As designed, it took the VM's lease and then the host lease, for the hidden Mac launches of every route. The skip is the Mac TestHooksTests, which runs through `tools/mg mac-ui`.
- Real runs in the VM, from this worktree, with the host unlocked. A watcher sampled the host every 5 s from 18:15 until xcode-check ended at 18:50. The host's seven `launchd_sim` processes (pool devices booted before U8c) did not change, and no other simulator booted on the host. Of this worktree's runs, only xcode-check took the host lease. During the runs, U12's worktree held the host lease (`bench-mac`, `perf-ui`, `xcode-check`), and the VM runs below did not wait for it. Every run below has `details.sim_runner: "vm"` (`mac_runner: "vm"` for mac-ui), and the VM lease waited 0 s.
  - `drive tools/drive-scripts/examples/open-sample-sheet.json --platform iphone` (20261015T161958Z-26188): 6 steps, no issue, 47 s.
  - `a11y --platform iphone` (20261015T162045Z-28565): 12 passed, 386 s, on Meguri-Pool-iphone-1.
  - `a11y --platform ipad` (20261015T162712Z-52990): 12 passed, 393 s, on Meguri-Pool-ipad-1.
  - `snapshots --platform iphone,ipad` (20261015T163344Z-89527): 152 pass, 44 deferred, 0 recorded, against the committed baselines. Settings groups: 9 on the iPhone, 4 on the small iPhone, 1 on the iPad. The approved-baseline guard passed.
  - `mac-ui --suite a11y` (20261015T163701Z-223): passed, 60 s, so U8b's VM path still works.
  - This pass did not repeat the runs with the host locked. The implementer's and the round 1 fixer's runs with the host locked are recorded in `docs/process/mac-ui-vm.md`. `tools/tests/sim_vm.bats` covers a locked host with a fake session.
- Provenance: `meguri-wt/U8c` was made by `tools/mg worktree new` (sparse `/*`, `!/oracle/`); no `oracle/` is on disk.
