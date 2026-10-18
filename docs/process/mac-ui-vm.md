# Mac UI tests and simulators in a VM

Antoine decided on 2026-10-15 that Mac UI-driving suites run in a macOS virtual machine on this Mac. Before, they ran on the real screen, at most once an hour. Plan unit U8b implements the change. The same day he extended it: everything that would disturb his Mac runs in the VM, as soon and as often as useful. Plan unit U8c moves the simulators there. This page records the VM as set up and verified on that day.

## The VM

- Tool: Tart 2.32.1 (Cirrus Labs, Developer ID signed and notarized), installed at `/Volumes/T9/meguri-vm/bin/tart-2.32.1/`. The wrapper `/Volumes/T9/meguri-vm/bin/tart` sets `TART_HOME=/Volumes/T9/meguri-vm/tart-home`, so every VM file lives on the external T9 drive (APFS, USB 10 Gb/s).
- VM `meguri-mac`: macOS 27.0.1 (26A434), the same build as the host, made from Apple's restore image (SHA-1 checked against Apple's catalog). 6 CPUs, 12 GB memory, 1920×1200 display, 150 GB disk.
- Inside the VM:
  - account `meguri`, with automatic login and passwordless sudo;
  - no sleep, no screen lock, no screen saver;
  - developer mode on, and automation mode without authentication;
  - Xcode 27.0 (27A266a) at `/Applications/Xcode.app`, license accepted, first launch done;
  - time zone Europe/Berlin, software updates download only.

  `tools/vm/setup-guest.sh` is the provisioning script, run once in the VM's Terminal.
- SSH: `/Volumes/T9/meguri-vm/bin/vm-ssh [command]`. It uses the VM's own key (`/Volumes/T9/meguri-vm/ssh/id_ed25519`) and takes the VM's address from `tart ip`. It passes `-F /dev/null`, because Antoine's `~/.ssh/config` makes plain `ssh` to the VM fail with "No route to host". Password login is off in the VM.
- Starting the VM: `tart run meguri-mac --no-graphics`, detached. It reaches a logged-in session about 20 s after boot. It keeps running while the host is locked. It never shows a window unless started without `--no-graphics`.

## How a test run works (verified by hand)

1. Build on the host as today: `tools/mg build-app --platform mac`. This writes `build/app/DerivedData/Build/Products` (35 MB) with `Meguri_UI_macosx27.0-arm64.xctestrun`.
2. Copy `Products` into the VM at the same absolute path, with `tar` over SSH. The UI tests read the app bundle at its build path (`app/MeguriUITests/Support/BuildUnderTest.swift`), and `/Users/antoine` in the VM belongs to `meguri`. Do not use Tart's shared folders (virtiofs): they break Xcode's symlinks ("Too many levels of symbolic links").
3. In the VM, run `xcodebuild test-without-building -xctestrun <Products>/Meguri_UI_macosx27.0-arm64.xctestrun -destination 'platform=macOS,arch=arm64' -resultBundlePath <dir>/x.xcresult -collect-test-diagnostics never -only-testing:MeguriUITests/<Class>`.
4. Copy the `.xcresult` and the log back.

Results on 2026-10-15, on feat/meguri-v1 at 5e24d8a:
- `AccessibilityHelperTests`: 11 passed, 1 skipped, in about 40 s.
- `TestHooksTests`: 5 passed. The 3 tests that synthesize input fail with the same macOS 27.0.1 bug as on the host (the `mac-input` user step):
  - WindowServer logs "the client with pid … is missing the 'com.apple.private.hid.client.event-dispatch' entitlement";
  - testmanagerd logs "Timed out after waiting 5.0s for Recap playback".

  In the VM, `log show` needs `sudo` to see those messages.

## How jobs use it (U8b)

The host's config, `~/Library/Application Support/meguri/mac-vm.json`, names the wrappers and the VM:

```json
{"tart": "/Volumes/T9/meguri-vm/bin/tart", "ssh": "/Volumes/T9/meguri-vm/bin/vm-ssh", "vms": ["meguri-mac"]}
```

`tools/mg mac-ui`, `tools/mg a11y --platform mac` and `tools/mg drive --platform mac` build on the host, take the VM's lease (one run at a time per VM, across checkouts), start the VM headless when it is stopped, and then follow the steps above. They copy `app/Fixtures` and a drive's script and fixtures too. They run as often as needed, with the host locked or not. Their `result.json` has `details.mac_runner: "vm"`. `tools/mg doctor` shows the VM's state. Conventions and traps: `tools/vm/AGENTS.md`.

On 2026-10-15, from the U8b worktree: `mac-ui --suite a11y` passed (11 passed, 1 skipped, 52 s with the incremental build), and so did `a11y --platform mac`. The full `mac-ui` passed agent-driver, a11y, mac-window-sizes and sample-routes. test-hooks (7 passed, 1 failed), open-sample-sheet and sample-transition ended `blocked: mac-input`, as on the host.

A test run in the VM outlives a lost SSH session or a killed job. So a job stops any leftover run (`xcodebuild test-without-building` and the UI-test runner) when it takes the VM, and right after it loses a session. If the run cannot be stopped, the job runs no more suites, and the VM gets no new run until a later job stops it. Verified on the real VM on 2026-10-15: the local SSH client of a running a11y suite was killed, and the job stopped the run in the VM before test-hooks started; a leftover process with a test run's command line was stopped before `a11y --platform mac` ran (11 passed, 1 skipped).

## Simulators (U8c)

### Provisioning (2026-10-15)

- The iOS 27.0 (24A434) simulator runtime came from this Mac, not from a download. Here it is a cryptex disk image of 8.07 GB (`Restore/043-70362-657.dmg` in its MobileAsset directory, `xcrun simctl runtime list -j` gives the path). Its directory went to `~/runtime-import/` in the VM by tar over SSH in 37 s, and `xcrun simctl runtime add` of that disk image took 47 s there. CoreSimulator cloned it into `/Library/Developer/CoreSimulator/Images/`, so the VM's disk use did not grow; the import directory was removed afterwards. `tools/vm/add-sim-runtime.sh` does these steps, and leaves a VM that already has the runtime alone (it did so on the provisioned VM). `tools/mg doctor` warns while the VM lacks it.
- `tools/mg sim-setup` then created the pool in the VM: `Meguri-Pool-iphone-1`, `-iphone-small-1`, `-ipad-1` and `-ipad-small-1` on the pinned runtime; the foldable lane is `unavailable: toolchain`, as on the host.
- Some SpringBoards ignore appearance changes: `simctl ui appearance dark` and `XCUIDevice.appearance = .dark` were accepted, and SpringBoard's style arbiter never logged them, so the accessibility audit's dark variants rendered light ("the variant's appearance did not apply"). It hit the iPhone's first boot and the iPad's SpringBoard restarted 45 s after its first boot; the small iPad's restart and a second small iPad's first boot were fine. A SpringBoard just restarted also drops changes for some seconds. A restart inside the simulator (`launchctl kickstart -k user/foreground/com.apple.SpringBoard` through `simctl spawn`) fixed each broken one. So once after every boot the pool switches the device to dark and back and looks for the arbiter's "changing style … to 2" in the simulator's log, and restarts SpringBoard (then waits 30 s) until it shows, at most three times. The device stays booted.

### How jobs use it

The simulator pool lives in the VM: `tools/scripts/sim-pool.sh` (and `tools/mg sim`) runs itself there over SSH, with its leases in the VM's `~/Library/Caches/meguri/sim-pool/`; a lease still names the host checkout. Every simulator job (`sim-setup`, `sim-reset`, `run`, `drive`, `ui-tree`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`, `perf-ui`, `metrics`) builds on the host as before, takes the VM's lease instead of the host lease, copies the products (and fixtures, drive scripts, snapshot baselines) to the same paths there, runs `simctl`, `devicectl`, `xcodebuild test-without-building` and `xctrace record` there, and copies result bundles, screenshots, videos, traces and snapshot output back into its run directory. Its `result.json` has `details.sim_runner: "vm"`. The simulators need no unlocked session on the host; only `xcode-check`'s hidden Mac launches do. Without the VM they are queued (warn, user step `mac-vm`); `MEGURI_SIM_ALLOW_HOST=1` uses the host's pool instead, for a run Antoine starts himself. Jobs that hold both the VM's lease and the host lease take the VM's first.

The VM runs one job at a time, and changing the pool counts: `sim acquire`, `sim gc` and `sim-setup` wait for the VM's lease, so no simulator is created, booted or deleted there while a suite runs. A script that calls `sim-pool.sh acquire` (or `gc`) directly gets the same: the script goes through `tools/mg sim`, unless it runs under the job that holds the lease (that job's own leases, `xcode-check`'s script). A launch fixture (`run -- -MeguriFixture <path>`, a drive script's `launch.fixture` or raw `arguments`) goes into the VM with the job: one in the checkout to its own path, one outside it under the run directory, with the launch naming that path. `xcode-check` copies the snapshot baselines, the test-hook fixtures and the example drive scripts with its products; a simulator run whose SSH session is lost is stopped in the VM before the next one starts, and when it cannot be stopped no further run starts there.

### Verified (2026-10-15, from the U8c worktree)

All runs went through the VM with `details.sim_runner: "vm"` and no host lease. From the first drive on, a watcher sampled the host every few seconds: the host's own seven pool simulators, booted before U8c, stayed as they were, no other simulator booted on the host, and none of these runs took the host lease. From 15:57 (local time) the host session was locked.

- `a11y --platform iphone` (20261015T124436Z-76381): 12 passed, 438 s with the incremental build.
- `drive tools/drive-scripts/examples/open-sample-sheet.json --platform iphone` (20261015T130907Z-51806): 6 steps, no issue, 84 s; the captures show the pinned status bar (9:41, full battery).
- `a11y --platform ipad`: the first run (20261015T131046Z-55871) failed the dark variants (the SpringBoard above). The second (20261015T133652Z-80265) got past them and timed out acquiring a background assertion for the app: the VM had booted four pool devices in the previous half hour and its load average had reached about 400. Once the load had dropped, the third (20261015T135902Z-23915) passed, 12 tests in 376 s, with the host locked.
- Snapshot comparison, `snapshots --platform iphone,ipad` (20261015T140536Z-33911), against the committed baselines: 152 pass, 44 deferred, no failing or new image, on the iphone, iphone-small and ipad kinds, with every settings group's probe present and applied (9 groups on the iPhone, 4 on the small iPhone, 1 on the iPad). Every image matched, so simulator snapshot runs stay in the VM; Mac snapshots render in-process on the host as before.
- `drive tools/drive-scripts/examples/sample-transition.json --platform iphone --record` (20261015T140934Z-45711): 5 steps, no issue; simctl recorded in the VM, the video came back and the contact sheet of the marked step was made from it. It first waited for the VM's lease, which a Mac UI performance run of another worktree held.
- A leased iPhone: `sim acquire`, `record start` (the worker recorded in the VM and held its lease), `run --route sample/sheet` and `screenshot --device` inside the recording's lease (nested), `record stop` (a 20.8 s video back in the run directory), `sim release`: all passed.
- `xcode-check` was not run for real: the host was locked, and its hidden Mac launches wait for an unlocked session. Its simulator runs in the VM are covered by `tools/tests/xcode_project_check.bats`.
- After review round 1, with the host locked:
  - `run --platform iphone -- -MeguriFixture <checkout>/build/vm-check/directory` (20261015T151556Z-10088): the fixture went to its own path in the VM, and the app opened on Home, not on the missing-fixture error (screenshot 20261015T151624Z-14310). The same with a fixture in the session's scratch directory, outside the checkout (20261015T151640Z-18089): it went to `vm-fixtures/1/` under the run directory, and the launch named that path. A fixture that does not exist ended the run as an error before anything launched.
  - `ui-tree --route sample/sheet -- -MeguriFixture <scratch file>` (20261015T151732Z-21266) passed through a copy of its script that names the VM's path. A direct `sim-pool.sh acquire ipad`, started while it ran, went through `tools/mg sim` (20261015T151749Z-22288), waited 31 s for the VM's lease and then leased the iPad.
  - `sim-setup` (20261015T151841Z-25447) waited 81 s for the VM's lease, which another job held, and passed.

## Limits

- The T9 drive must be connected and mounted. Without it, there is no VM.
- Timings in the VM do not represent the hardware. Mac and iOS UI performance suites (`ui-perf`, U12) run in the VM against baselines recorded there; CPU benchmarks without UI (`bench-mac`) stay on the host.
- One VM holds the simulator pool (the first configured VM that can run). A second VM adds capacity for Mac UI suites only.
- The VM has 6 CPUs and 12 GB of memory. Pool devices stay booted (five on 2026-10-15), and a device's first boots load the VM for several minutes: a suite that runs meanwhile can time out. More CPUs and memory (`tart set meguri-mac --cpu … --memory …`, with the VM stopped) would give simulators more room.
- The host cannot record the VM's screen: Mac `drive --record` in the VM makes its contact sheets from screenshots the test runner samples.
- Input-driving Mac tests fail in the VM until Apple fixes the macOS 27.0.1 bug, exactly as on the host.

### Mac input repair (2026-10-18)

The earlier mac-input diagnosis applied to touch events: `.tap()` and coordinate `.press()` took Recap's touch path. The VM's `/usr/libexec/testmanagerd` has `com.apple.private.hid.client.event-dispatch.internal`, which WindowServer rejects on a non-internal build. Automation Mode does enable during XCTest, and no OS update was available. Native mouse events work on the same VM and OS/Xcode pair. Shared UI tests and the agent driver now use `clickOrTap()` (mouse on macOS, touch on iOS), and Mac drags use `click(forDuration:thenDragTo:)`. The alert-dismiss test passed with a real click (ui-test 20261018T065638Z-65888); the six-step sheet driver passed with screenshots and hierarchy checks (drive 20261018T065901Z-70138). No host input was driven and no security setting was weakened.
