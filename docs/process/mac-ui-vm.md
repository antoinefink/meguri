# Mac UI tests in a VM

Antoine decided on 2026-10-15 that Mac UI-driving suites run in a macOS virtual machine on this Mac. Before, they ran on the real screen, at most once an hour. Plan unit U8b implements the change. This page records the VM as set up and verified on that day.

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

## Limits

- The T9 drive must be connected and mounted. Without it, there is no VM.
- Timings in the VM do not represent the hardware. Mac UI performance suites (`ui-perf --platform mac`, U12) run in the VM against baselines recorded there; CPU benchmarks without UI (`bench-mac`) stay on the host.
- The host cannot record the VM's screen: Mac `drive --record` in the VM makes its contact sheets from screenshots the test runner samples.
- Input-driving Mac tests fail in the VM until Apple fixes the macOS 27.0.1 bug, exactly as on the host.
