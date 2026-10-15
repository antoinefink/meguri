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

## Limits

- The T9 drive must be connected and mounted. Without it, there is no VM.
- Timings in the VM do not represent the hardware: performance jobs (`ui-perf`, `bench-mac`) stay on the host.
- Input-driving Mac tests fail in the VM until Apple fixes the macOS 27.0.1 bug, exactly as on the host.
