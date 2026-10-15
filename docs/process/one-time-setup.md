# One-time setup (user steps)

These steps change system settings, accept license terms, or sign in. Only Antoine performs them; agents never do (R1969). `tools/mg doctor` checks what it can and lists the rest in its result under `user_steps`. Each heading below is a step id.

- `missing`: doctor checked and the step is not done.
- `unverified`: doctor cannot check the step from the command line.

A missing required step makes doctor `warn`, or `fail` when it blocks all Apple builds. Optional steps never change doctor's status.

## Required

### `xcode-select`

Select Xcode as the active developer directory: run `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`. Without it, `xcodebuild` and `simctl` do not work. Doctor fails.

### `xcode-license`

Accept the Xcode license: open Xcode once, or run `sudo xcodebuild -license accept`. Doctor fails until it is accepted.

### `ios-runtime`

Install the iOS 27 simulator runtime: Xcode › Settings › Components, or `xcodebuild -downloadPlatform iOS`. The iPhone and iPad simulators share it. Doctor fails without it.

### `xcode-account`

Add the Apple account for team DNQ34W43XX: Xcode › Settings › Accounts › + › Apple Account. Until then, Mac builds are ad-hoc signed (no App Group) and device builds are blocked. Doctor warns.

### `docker-resources`

Give Docker Desktop at least 12 CPUs and 16 GB of memory: Docker Desktop › Settings › Resources. The oracle runs 12 workers. Doctor warns below these values. It reads the memory total inside Docker's Linux VM, which is up to about 1 GB below the setting, so it accepts 90% of 16 GB.

### `uitest-automation`

Let the Xcode UI-test runner control the Mac. On the first macOS UI-test run, macOS asks; allow it in System Settings › Privacy & Security › Accessibility and Automation. Doctor cannot verify this. Mac UI suites run in the test VM, where `tools/vm/setup-guest.sh` turns on automation mode without authentication; this Mac needs the step only for host runs (`MEGURI_MAC_UI_ALLOW_HOST=1`).

### `mac-vm`

Keep the T9 drive with the Mac UI test VM connected. Every Mac UI suite (`tools/mg mac-ui`, `a11y --platform mac`, `drive --platform mac`) runs in that macOS VM, never on this Mac's screen (`docs/process/mac-ui-vm.md`). The host names it in `~/Library/Application Support/meguri/mac-vm.json`. Without the config, the drive or the VM, the suites are queued (status warn). Doctor checks the config, the drive, the VM, SSH and the VM's Xcode.

### `mac-input`

Use a macOS and Xcode pair whose Mac UI tests can click and type. On macOS 27.0.1 (26A434) with Xcode 27.0, WindowServer refuses the input service of `testmanagerd` (log: "missing the 'com.apple.private.hid.client.event-dispatch' entitlement"). The tests start, but no tap, key press or drag reaches the app. No permission fixes this: update macOS or Xcode. The test VM runs the same macOS build and has the same bug. Until then, `tools/mg mac-ui` and `tools/mg drive --platform mac` report `blocked: mac-input` (status error) when the unified log shows the dropped input. Mac suites without input still pass. Doctor cannot verify this.

## Optional or later

### `device-developer-mode`

Enable Developer Mode on each iPhone and iPad used for milestone device runs: Settings › Privacy & Security › Developer Mode. Doctor cannot verify this.

### `screen-recording`

Allow Screen Recording for the app that runs `tools/mg` (the terminal, or Claude) in System Settings › Privacy & Security › Screen Recording. `tools/mg screenshot --platform mac` needs it to capture the hidden Mac app's window. Without it the job reports blocked, and Mac screenshots come only from `tools/mg drive --platform mac` (in the test VM). Mac drives in the VM make motion contact sheets from XCUITest screenshots at about 10 per second; only a host run (`MEGURI_MAC_UI_ALLOW_HOST=1`) with this permission records the screen as video. Doctor cannot verify this.

### `xcode-external-agents`

Optional: enable "Allow external agents to use Xcode tools" in Xcode's settings. Doctor cannot verify this.

### `xcode-27-1`

Install Xcode 27.1 when it ships. It adds the foldable iPhone simulator (R1962); until then, foldable layouts are approximated. Doctor lists this step while Xcode is older than 27.1.

### `nightly-scheduler`

Approve the scheduler entry for nightly jobs when `tools/mg nightly` (U19) asks for it. There is no git remote, so nightly runs happen on this Mac only. Doctor cannot verify this.

## While agents run

### `host-session`

Not a one-time step. App and simulator jobs (`build-app` excepted) wait while the Mac is locked or its display sleeps. They report `blocked: host` and run again later, so UI gates wait too. While they run, they keep the display awake; a simulator recording (`tools/mg record start`) does so until it is stopped, at most 10 minutes. Mac UI suites run in the test VM and need no unlocked session; only a host run (`MEGURI_MAC_UI_ALLOW_HOST=1`) waits, and takes the screen at most once an hour.

## Not user steps

Doctor also reports these, but an agent may fix them:

- Rust toolchain and Apple targets: `cd core && rustup toolchain install` installs what `core/rust-toolchain.toml` lists.
- Docker not running: start Docker Desktop.
- Low disk space: doctor warns below 40 GiB free and fails below 10 GiB.
