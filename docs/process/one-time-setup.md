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

Install the iOS 27 simulator runtime: Xcode › Settings › Components, or `xcodebuild -downloadPlatform iOS`. The iPhone and iPad simulators share it. Builds for the simulator need it on this Mac, and the test VM's simulators get their copy from it (`tools/vm/add-sim-runtime.sh`, run by an agent). Doctor fails without it.

### `xcode-account`

Add the Apple account for team DNQ34W43XX: Xcode › Settings › Accounts › + › Apple Account. Until then, Mac builds are ad-hoc signed (no App Group) and device builds are blocked. Doctor warns.

### `docker-resources`

Give Docker Desktop at least 12 CPUs and 16 GB of memory: Docker Desktop › Settings › Resources. The oracle runs 12 workers. Doctor warns below these values. It reads the memory total inside Docker's Linux VM, which is up to about 1 GB below the setting, so it accepts 90% of 16 GB.

### `uitest-automation`

Let the Xcode UI-test runner control the Mac. On the first macOS UI-test run, macOS asks; allow it in System Settings › Privacy & Security › Accessibility and Automation. Doctor cannot verify this. Mac UI suites run in the test VM, where `tools/vm/setup-guest.sh` turns on automation mode without authentication; this Mac needs the step only for host runs (`MEGURI_MAC_UI_ALLOW_HOST=1`).

### `mac-vm`

Keep the T9 drive with the macOS test VM connected. Every Mac UI suite (`tools/mg mac-ui`, `a11y --platform mac`, `drive --platform mac`) and every simulator (`sim`, `run`, `drive`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`) runs in that VM, never on this Mac (`docs/process/mac-ui-vm.md`). The host names it in `~/Library/Application Support/meguri/mac-vm.json`. Without the config, the drive or the VM, those jobs are queued (status warn). So are they while the VM is there but cannot take them: it does not answer over SSH in a logged-in session, or its user is near the process limit (2,900 processes; each booted pool simulator holds about 235). That usually clears once its load drops; if it does not, look at the VM (`docs/process/mac-ui-vm.md`): jobs never stop or restart it. Doctor checks the config, the drive, the VM, SSH, the logged-in session, the free processes, the VM's Xcode and its simulator runtime, and lists this step while any of them keeps the jobs queued; `tools/mg catch-up` runs them again once it does not.

### `mac-input`

Use a macOS and Xcode pair whose Mac UI tests can click and type. On macOS 27.0.1 (26A434) with Xcode 27.0, WindowServer refuses the input service of `testmanagerd` (log: "missing the 'com.apple.private.hid.client.event-dispatch' entitlement"). The tests start, but no tap, key press or drag reaches the app. No permission fixes this: update macOS or Xcode. The test VM runs the same macOS build and has the same bug. Until then, a Mac UI run (`tools/mg mac-ui`, `a11y --platform mac`, `drive --platform mac`, `perf-ui --platform mac`) whose unified log shows the dropped input is queued (status warn, `blocked: mac-input`); the suites without input still pass or fail. The run records where it saw the drop, with that runner's macOS and Xcode builds (`build/mg/mac-input.json` in the main checkout). Doctor lists the step while the runner still has both builds, or while it cannot read them (a VM that is not running), and clears it once either changed; `tools/mg catch-up` then runs the queued runs again.

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

Approve the scheduler entries for the nightly and weekly runs. `tools/mg nightly --print-schedule` (U19) writes them, `com.meguri.nightly.plist` and `com.meguri.nightly.weekly.plist`, under its run directory; copy them to `~/Library/LaunchAgents/` and load them with `launchctl load`. There is no git remote, so nightly runs happen on this Mac only. `tools/mg nightly` lists the step as missing while the nightly entry is not in `~/Library/LaunchAgents/`; doctor lists it as unverified.

## While agents run

### `host-session`

Not a one-time step. Jobs that launch the Mac app on this Mac (`run --platform mac`, `screenshot --platform mac`, the hidden launches of `xcode-check`, the Mac part of `snapshots`) are queued while the Mac is locked or its display sleeps (status warn, `blocked: host`); the parts that ran keep their results. Doctor lists the step while it holds, and `tools/mg catch-up` runs the queued runs of merged units again once it clears. While they run, they keep the display awake. Simulator jobs and Mac UI suites run in the test VM and need no unlocked session; only a host run (`MEGURI_SIM_ALLOW_HOST=1`, `MEGURI_MAC_UI_ALLOW_HOST=1`) waits, and a Mac UI host run takes the screen at most once an hour.

## Not user steps

Doctor also reports these, but an agent may fix them:

- Rust toolchain and Apple targets: `cd core && rustup toolchain install` installs what `core/rust-toolchain.toml` lists.
- Docker not running: start Docker Desktop.
- Low disk space: doctor warns below 40 GiB free and fails below 10 GiB.
