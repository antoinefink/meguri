# Mac UI tests and simulators in a VM

Current topology (2026-11-24): M5 is the sole MG scheduler and build
coordinator. The dedicated headless `meguri-mac` VM lives on T9 attached to
M3 Max. The private coordinator wrappers use the kit at `/Volumes/T9/Meguri VM`
on the M3 Max. See
[`new-machine.md`](new-machine.md#current-coordinator-and-remote-test-vm) for
atomic installation, removal of old activation overrides and verification.
The provisioning and dated results below describe the original installation.

Antoine decided on 2026-10-15 that Mac UI-driving suites run in a macOS virtual machine on this Mac. Before, they ran on the real screen, at most once an hour. Plan unit U8b implements the change. The same day he extended it: everything that would disturb his Mac runs in the VM, as soon and as often as useful. Plan unit U8c moves the simulators there. This page records provisioning and subsequent verification. The 2026-10-19 supplement below moves the remaining Mac app-hosted checks into the VM.

## Recovery permission (2026-10-22)

The orchestrator may stop that VM once no test is running and start it through
the configured coordinator `tart` wrapper with `run --no-graphics meguri-mac`.
The remote wrapper detaches that exact MG headless form on the hosting Mac;
its private log path is printed to stderr. Do not launch a second local guest.
Use `tools/mg doctor` to check readiness, then `tools/mg catch-up` for queued
checks. Keep the VM running headlessly. This permission covers the Meguri test
VM; it does not change the host-screen or other-project data rules.

## Original VM provisioning (October 2026)

- Tool: Tart 2.32.1 (Cirrus Labs, Developer ID signed and notarized), installed at `/Volumes/T9/meguri-vm/bin/tart-2.32.1/`. The wrapper `/Volumes/T9/meguri-vm/bin/tart` sets `TART_HOME=/Volumes/T9/meguri-vm/tart-home`, so every VM file lives on the external T9 drive (APFS, USB 10 Gb/s).
- VM `meguri-mac`: macOS 27.0.1 (26A434), the same build as the host, made from Apple's restore image (SHA-1 checked against Apple's catalog). 6 CPUs, 12 GB memory, 1920×1200 display, 150 GB disk.
- Inside the VM:
  - account `meguri`, with automatic login and passwordless sudo;
  - no sleep, no screen lock, no screen saver;
  - developer mode on, and automation mode without authentication;
  - Xcode 27.0 (27A266a) at `/Applications/Xcode.app`, license accepted, first launch done (since replaced as the selected Xcode: see [iOS 27.2](#ios-272-2026-11-19));
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

The coordinator's default per-user config,
`~/Library/Application Support/meguri/mac-vm.json`, names its stable private
`remote-vm/current/tart` and `remote-vm/current/vm-ssh` executable files and
`vms: ["meguri-mac"]`, using absolute paths. Install it as documented in
[`new-machine.md`](new-machine.md#install-the-coordinator-wrappers), clearing
old activation overrides in every shell. The SSD paths above are used only
on the hosting Mac. Manual VNC needs a separate tunnel; the remote launcher
does not detach VNC sessions or rewrite their addresses.

`tools/mg mac-ui`, `tools/mg a11y --platform mac` and `tools/mg drive --platform mac` build on the host, take the VM's lease (one run at a time per VM, across checkouts), start the VM headless when it is stopped, and then follow the steps above. They copy `app/Fixtures` and a drive's script and fixtures too. They run as often as needed, with the host locked or not. Their `result.json` has `details.mac_runner: "vm"`. `tools/mg doctor` shows the VM's state. Conventions and traps: `tools/vm/AGENTS.md`.

On 2026-10-15, from the U8b worktree: `mac-ui --suite a11y` passed (11 passed, 1 skipped, 52 s with the incremental build), and so did `a11y --platform mac`. The full `mac-ui` passed agent-driver, a11y, mac-window-sizes and sample-routes. test-hooks (7 passed, 1 failed), open-sample-sheet and sample-transition ended `blocked: mac-input`, as on the host.

A test run in the VM outlives a lost SSH session or a killed job. So a job stops any leftover run (`xcodebuild test-without-building` and the UI-test runner) when it takes the VM, and right after it loses a session. If the run cannot be stopped, the job runs no more suites, and the VM gets no new run until a later job stops it. Verified on the real VM on 2026-10-15: the local SSH client of a running a11y suite was killed, and the job stopped the run in the VM before test-hooks started; a leftover process with a test run's command line was stopped before `a11y --platform mac` ran (11 passed, 1 skipped).

## Simulators (U8c)

### Provisioning (2026-10-15)

- The iOS 27.0 (24A434) simulator runtime came from this Mac, not from a download. Here it is a cryptex disk image of 8.07 GB (`Restore/043-70362-657.dmg` in its MobileAsset directory, `xcrun simctl runtime list -j` gives the path). Its directory went to `~/runtime-import/` in the VM by tar over SSH in 37 s, and `xcrun simctl runtime add` of that disk image took 47 s there. CoreSimulator cloned it into `/Library/Developer/CoreSimulator/Images/`, so the VM's disk use did not grow; the import directory was removed afterwards. `tools/vm/add-sim-runtime.sh` does these steps, and leaves a VM that already has the runtime alone (it did so on the provisioned VM). `tools/mg doctor` warns while the VM lacks it.
- `tools/mg sim-setup` then created the pool in the VM: `Meguri-Pool-iphone-1`, `-iphone-small-1`, `-ipad-1` and `-ipad-small-1` on the pinned runtime; the foldable lane ran on its own iOS 27.1 runtime (`tools/vm/add-sim-runtime.sh --foldable`). The pin has moved since: see [iOS 27.2](#ios-272-2026-11-19).
- Some SpringBoards ignore appearance changes: `simctl ui appearance dark` and `XCUIDevice.appearance = .dark` were accepted, and SpringBoard's style arbiter never logged them, so the accessibility audit's dark variants rendered light ("the variant's appearance did not apply"). It hit the iPhone's first boot and the iPad's SpringBoard restarted 45 s after its first boot; the small iPad's restart and a second small iPad's first boot were fine. A SpringBoard just restarted also drops changes for some seconds. A restart inside the simulator (`launchctl kickstart -k user/foreground/com.apple.SpringBoard` through `simctl spawn`) fixed each broken one. So once after every boot the pool switches the device to dark and back and looks for the arbiter's "changing style … to 2" in the simulator's log, and restarts SpringBoard (then waits 30 s) until it shows, at most three times. The device stays booted.

### iOS 27.2 (2026-11-19)

Antoine decided on 2026-11-19 to stay on the latest versions, VM included. `tools/sim-devices.json` now pins iOS 27.2 (24B8117) for the four standard kinds and the iPhone Duo's own build of it (24B91016s, `…iOS-27-2-iPhone19-4`). The standard iPhone is the iPhone 18 Pro and the small iPad the iPad mini (A20 Pro); the small iPhone (17e) and the iPad Pro 13-inch (M5) keep their device types.

- Xcode: the VM selects Xcode 27.2 (27B52) at `~/Applications/MeguriXcode/Xcode-27.2-27B52.app`, staged by `tools/mg vm-xcode-stage`. Xcode 27.0 stays at `/Applications/Xcode.app`. The VM's macOS is still 27.0.1 (26A434).
- Runtimes: both 27.2 images came from this Mac with `tools/vm/add-sim-runtime.sh` and `tools/vm/add-sim-runtime.sh --foldable`, as in 2026-10-15. The iOS 27.0 (24A434) and 27.1 (24A94425) runtimes are still installed: checkouts on the old pin use them until this change lands.
- Pool: `sim-setup` only adds. It creates a device of each kind on the pinned runtime under the next free number and leaves the devices of the old runtimes as they are, booted ones included; `sim status` marks those `(old runtime)`, and no job on the new pin leases them. The first run (`sim-setup/20261119T192921Z-92482`) created `Meguri-Pool-iphone-4` and `-ipad-4` (booted, reset and released), `-iphone-small-3` (its first boot failed: "launchd_sim may have crashed or quit responding") and `-ipad-small-3` (booted; the appearance check after its boot never returned). It was cancelled there, before the foldable kind.
- Capacity: with four devices of the old pool still booted, the three new ones brought the VM (12 GB) to seven booted simulators. Its load average passed 600, `simctl` calls stopped returning and SSH commands took minutes. The old devices have to be shut down or deleted (or the VM restarted) before the remaining kinds can boot; a job never does that by itself.

### How jobs use it

The simulator pool lives in the VM: `tools/scripts/sim-pool.sh` (and `tools/mg sim`) runs itself there over SSH, with its leases in the VM's `~/Library/Caches/meguri/sim-pool/`; a lease still names the host checkout. Every simulator job (`sim-setup`, `sim-reset`, `run`, `drive`, `ui-tree`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`, `perf-ui`, `metrics`) builds on the host as before, takes the VM's lease instead of the host lease, copies the products (and fixtures, drive scripts, snapshot baselines) to the same paths there, runs `simctl`, `devicectl`, `xcodebuild test-without-building` and `xctrace record` there, and copies result bundles, screenshots, videos, traces and snapshot output back into its run directory. Its `result.json` has `details.sim_runner: "vm"`. No runtime check in `xcode-check` or `snapshots` needs an unlocked host session: their hidden Mac launches and app-hosted plans also run in the VM. Without the VM they are queued (warn, user step `mac-vm`); `MEGURI_SIM_ALLOW_HOST=1` uses the host's pool instead for other simulator jobs that Antoine starts himself; snapshot, fixture-launch and Xcode runtime checks always require the VM. Jobs that need both leases take the VM's first.

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

## Remaining Mac hosted checks (2026-10-19)

Fixture argument launches, Mac snapshots, Xcode's hidden Debug launch probes and its Mac Unit/Snapshot plans now execute under the existing VM lease. Builds stay on the host. Missing VM capacity queues runtime work; it does not launch the app on the host. The wrappers suppress host fallback variables. Xcode copies current products and runtime inputs, including the core manifest and color assets, before testing; missing inputs fail instead of reusing an older copy. Hidden launches retain their liveness, Dock and foreground checks, and cancellation stops app hosts and test processes before releasing the VM.

The first full Mac render used the VM's actual key, `VirtualMac2,1-26A434`, which had no references. A controlled comparison against the existing `Mac15,9-26A434` references passed 56 of 60 cells (`snapshots/20261019T163541Z-6266`). Four dark cells differed at the unchanged strict tolerances; their fractions and review are recorded in `docs/process/review-log/U8b.md`. Antoine approved a separate VM reference set in chat on 2026-10-19. This is a migration exception to U8c's original host-only Mac snapshot placement, not a tolerance change or approval of future differences.

`tools/mg snapshots --platform mac --record '*'` recorded 60 files only under `app/Snapshots/VirtualMac2,1-26A434/` (`20261019T171900Z-56838`, committed in `5a037342`). Host references and the signature approval ledger were preserved. A fresh unfiltered verification without record mode or a reference-key override passed all 60 comparisons on clean `5a037342` (`20261019T171952Z-59085`); the test bundle reported 63 passed, zero failed and one existing skip. The final unit gate remains required before accepting the migration.

The first full gate exposed an existing native audio probe that still assumed host hardware. With no audio route, its play promise timed out and the following video navigation stalled. The native MP3 test now uses the same real-hardware condition as the UI audio test, before creating a page. Video stays enabled. A focused host control on clean `baaa7a2d` passed all eight native media tests, including audio and video, with no skips (`swift-test/20261019T174510Z-20111`). The full VM Unit plan must confirm the audio skip and successful video execution. Physical iOS S2(a) and the recorded S1(f) footprint issue remain open.

Keep host Apple jobs serial. Swift package tests and Xcode builds share the generated core framework across configurations. In the first gate, a parallel Debug package build replaced that framework while the AppStore build read its stamp; the configuration safety check correctly failed. Rust and image checks may overlap Apple work, but the strict XCFramework timing selftest runs alone on a quiet machine.

## The Mac app on the host (U8d)

Before the 2026-10-19 migration, two paths started the Mac app on this Mac: app-hosted tests (including Mac snapshots) and explicit hidden launches through `tools/mg run` and `screenshot`. The app-hosted tests now run in the VM; explicit host run/capture jobs retain the hidden-launch safeguards below. On 2026-10-16 Antoine saw the app in his Dock while tests ran, and chose this fix:

- The Debug app declares `LSUIElement`. LaunchServices registers a launch from the Info.plist before any app code runs, so a regular app is reported `Foreground`, with a Dock tile, from its first moment; a policy set at run time alone would come too late.
- `MeguriApp.init` decides once, before the app finishes launching (`app/Meguri/TestHooks/BackgroundLaunch.swift`). A test host (`XCTestConfigurationFilePath` set) or a launch with `-MeguriBackgroundLaunch YES` stays accessory: no Dock tile, never active, and its windows still render off-screen and exist for `screencapture -l`. Any other launch, by hand or by XCUIApplication in the VM's Mac UI suites, takes the regular policy.
- `tools/mg run` and `screenshot` pass the argument and fail if `lsappinfo` reports the app `Foreground` or in front. They launch only Debug on this Mac. Profile, Release and AppStore have no switch and start as regular apps (`LSUIElement` false); Profile keeps Release's launch for performance runs (R1866).

Verified on 2026-10-17 from the U8d worktree, with a watcher that polled `lsappinfo list` about three times a second for apps built there:

- `snapshots --platform mac` (20261017T174219Z-60935): 60 pass against the committed references, none new or changed. The test host was `UIElement` and never in front for its whole run. Before the fix, the hosted test that checks this failed: the host had the regular policy.
- `run --platform mac --route sample/screen --seconds 8` (20261017T174331Z-63793): passed, `UIElement` and never in front. During a second run (20261017T174422Z-66727) the hidden app's window (1180×780, layer 0, off screen) was in the window list, which is what `screencapture -l` captures.
- `screenshot --platform mac` (20261017T174357Z-64876) was blocked before launching: the session that ran it has no Screen Recording permission (user step `screen-recording`).

## Fold positions (2026-11-01)

### Automated leased Duo posture

`tools/mg sim-posture <leased-udid> open|closed` requests Device Hub's native
posture control in the already-running dedicated pool VM. MG prepares Mac
Debug UI products before taking the VM lease; no build runs inside XCTest.
It never starts a VM/simulator or uses workstation fallbacks, VNC or trust setup.

MG checks the checkout-owned simulator lease, pinned runtime **build**, unique
simulator name, booted inventory and iPhone Duo type. After product delivery it
requires more than 120 seconds on the unchanged simulator lease. The request
allows input for at most 90 seconds, with five seconds reserved at the click;
XCTest has a 30-second execution allowance. MG's normal login admission and
runner's hypervisor/`meguri` account checks remain in effect. The caller retains
and releases the simulator lease.

The sandboxed runner uses only supported `XCUIApplication` queries and native
clicks to interact with Device Hub. It tries a uniquely named simulator display
window, then a unique row in a Simulators outline/group and matching details in
the same library window. Inspector need not display a UUID. Ambiguous targets
or missing controls queue with fixed diagnostic counts (Hub/target windows,
simulator rows, detail panes and named controls); no arbitrary UI labels are
exported. No physical-device row is selected. Closed, Partially Open and Open
must be distinct controls with an exclusive selected state. An already-selected
posture needs no click. A posture click is never retried.

The runner attaches `mg-posture.json` with `.keepAlways`. MG receives the normal
result bundle and selectively exports exactly one matching method attachment,
checking its target and per-run nonce. The runner reads no checkout or lease
files, launches no subprocesses, writes no report path and needs no direct AX
trust. MG checks the lease again after the receipt. Outside XCTest, MG uses its
guest simctl helper to capture displays 1 and 3 twice and a small guest AppKit
script to reduce pixels to dimensions/content flags; PNGs are removed in the
guest without transferring them. Closed requires outer content and black inner;
Open requires the reverse. Both lit/blank or a mismatch fails verification.
The UI's exclusive selection also distinguishes Open from Partially Open.

Require `.status == "pass"` and `.details.display_verified == true` before
`drive --platform foldable --device <udid>`; queued jobs also exit zero. Focused
checks: `tools/mg selftest tools/tests/sim_posture.bats tools/tests/sim_vm.bats
--filter sim-posture`. The action skips in ordinary UI runs without MG's request.

The supplied initial native guest invocation reached XCTest but failed writing
`posture.json` in MG's workspace with `NSCocoaErrorDomain 513`. This is evidence
of the blocked file transport, not live posture acceptance. This revision has
no native build or live acceptance result from the filtered runner. The
integrator must check compact and expanded Hub windows, Closed → Open → Closed,
repeated requests and a Partially Open start on the selected guest Xcode.

Only Device Hub's buttons change the iPhone Duo's fold position. `tools/vm/duo-fold.sh` reaches them from the headless VM over Tart's VNC server: `start`, then `tools/mg sim acquire foldable`, `hub <udid>`, `fold closed|half|open`, `shot <udid> <display> <file>` and `stop`. Display 1 is outer and 3 is inner. `tools/mg` jobs still work during the session.

Keep one VNC connection. Reconnecting for each click crashed Tart twice (an assertion in `_VZVirtualMachineAccessor addAccessorObserver`) and took down what ran in the VM; `tools/vm/vnc-holder.py` holds the connection for the whole session. A restart shuts the simulators down and voids their leases.

## Growing the disk (2026-11-01)

The disk went from its first size to 300 GB. `tart set meguri-mac --disk-size 300` with the VM stopped grows the image, but the recovery partition after the system container blocks the container. macOS refuses to remove it from the host, and in the guest while SIP is on. Recovery mode can:

1. Boot with `tart run meguri-mac --recovery --no-graphics --vnc-experimental`, and open Terminal over VNC.
2. Check that `diskutil list` shows `disk0s3` as `Apple_APFS_Recovery`. Stop here if it does not.
3. Run `diskutil eraseVolume free free disk0s3`, `yes | diskutil repairDisk disk0` and `diskutil apfs resizeContainer disk0s2 0`, then shut down.

The partition stays removed. Recovery mode has not been booted since, so a second growth may need another route.

## Limits

- T9 must be connected and mounted on M3 Max, and coordinator SSH must reach it. Without either, VM work queues with host fallbacks disabled.
- Timings in the VM do not represent the hardware. Mac and iOS UI performance suites (`ui-perf`, U12) run in the VM against baselines recorded there; CPU benchmarks without UI (`bench-mac`) stay on the host.
- One VM holds the simulator pool (the first configured VM that can run). A second VM adds capacity for Mac UI suites only.
- The VM has 6 CPUs and 12 GB of memory. Pool devices stay booted (five on 2026-10-15), and a device's first boots load the VM for several minutes: a suite that runs meanwhile can time out. More CPUs and memory (`tart set meguri-mac --cpu … --memory …`, with the VM stopped) would give simulators more room.
- The host cannot record the VM's screen: Mac `drive --record` in the VM makes its contact sheets from screenshots the test runner samples.
- Mac touch-event synthesis fails on macOS 27.0.1; the native mouse-event repair below allows the UI suites to run in the VM.

### Mac input repair (2026-10-18)

The earlier mac-input diagnosis applied to touch events: `.tap()` and coordinate `.press()` took Recap's touch path. The VM's `/usr/libexec/testmanagerd` has `com.apple.private.hid.client.event-dispatch.internal`, which WindowServer rejects on a non-internal build. Automation Mode does enable during XCTest, and no OS update was available. Native mouse events work on the same VM and OS/Xcode pair. Shared UI tests and the agent driver now use `clickOrTap()` (mouse on macOS, touch on iOS), and Mac drags use `click(forDuration:thenDragTo:)`. The alert-dismiss test passed with a real click (ui-test 20261018T065638Z-65888); the six-step sheet driver passed with screenshots and hierarchy checks (drive 20261018T065901Z-70138). No host input was driven and no security setting was weakened.
