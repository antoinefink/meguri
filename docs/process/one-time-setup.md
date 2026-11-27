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

Install the pinned iOS simulator runtime (`tools/sim-devices.json`; iOS 27.2 since 2026-11-19): Xcode › Settings › Components, or `xcodebuild -downloadPlatform iOS`. The iPhone and iPad simulators share it. Builds for the simulator need it on this Mac, and the test VM's simulators get their copy from it (`tools/vm/add-sim-runtime.sh`, run by an agent). Doctor fails without it.

### `xcode-account`

Add the Apple account for team DNQ34W43XX: Xcode › Settings › Accounts › + › Apple Account. Doctor reads legacy team preferences. If they do not list the team, it warns and leaves account setup unverified. Verify the account in Xcode; a team-signed build checks current signing and provisioning for its destination.

Physical development builds also need current account agreements and App Group
`group.com.antoinefink.meguri.dev` assigned to both App IDs
`com.antoinefink.meguri.dev` and `com.antoinefink.meguri.dev.widget`.
Configure these in Certificates, Identifiers & Profiles. Doctor checks only
whether the account is present; it does not establish profile validity.
`ui-test --physical-device <identifier>` uses team signing and lets Xcode refresh
profiles. Preserve the app and widget's shared-group entitlements.

### Retired iCloud setup (2026-11-27)

`cloudkit-setup` and `cloudkit-test-accounts` are retired. Do not enable iCloud
or push capabilities, configure CloudKit schemas, approve test accounts, or run
the removed setup/signing-matrix jobs for Meguri. Earlier setup records and
receipts remain historical evidence. Meguri Sync is the only supported sync
service; Off remains supported. Existing profiles may authorize unused iCloud
or push capabilities: packaging checks require those grants to be absent from
the actual app and widget signatures and both products' Info and linkage.

### `docker-resources`

Give Docker Desktop at least 12 CPUs and 16 GB of memory: Docker Desktop › Settings › Resources. The oracle runs 12 workers. Doctor warns below these values. It reads the memory total inside Docker's Linux VM, which is up to about 1 GB below the setting, so it accepts 90% of 16 GB.

### `uitest-automation`

Let the Xcode UI-test runner control the Mac. On the first macOS UI-test run, macOS asks; allow it in System Settings › Privacy & Security › Accessibility and Automation. Doctor cannot verify this. Mac UI suites run in the test VM, where `tools/vm/setup-guest.sh` turns on automation mode without authentication; this Mac needs the step only for host runs (`MEGURI_MAC_UI_ALLOW_HOST=1`).

### `mac-vm`

Keep the configured macOS test VM’s storage available, either in the ignored `.local/meguri-vm/` directory or on an external drive (`new-machine.md`). Every Mac UI suite (`tools/mg mac-ui`, `a11y --platform mac`, `drive --platform mac`) and every simulator (`sim`, `run`, `drive`, `screenshot`, `record`, `a11y`, `snapshots`, `xcode-check`) runs in that VM, never on this Mac (`docs/process/mac-ui-vm.md`). The host names it in `~/Library/Application Support/meguri/mac-vm.json`. Without the config, the drive or the VM, those jobs are queued (status warn). So are they while the VM is there but cannot take them: it does not answer over SSH in a logged-in session, or its user is near the process limit (2,900 processes; each booted pool simulator holds about 235). That usually clears once its load drops; if it does not, look at the VM (`docs/process/mac-ui-vm.md`): jobs never stop or restart it. Doctor checks the config, the drive, the VM, SSH, the logged-in session, the free processes, the VM's Xcode and its simulator runtime, and lists this step while any of them keeps the jobs queued; `tools/mg catch-up` runs them again once it does not.

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

Xcode 27.1 or later adds the iPhone Duo simulator (R1962). It runs on its own device-specific build of the pinned iOS version (iOS 27.2 since 2026-11-19, which needs Xcode 27.2), pinned under `foldable` in `tools/sim-devices.json`; `tools/vm/add-sim-runtime.sh --foldable` copies it into the VM. Doctor lists this step while Xcode is older than 27.1.

### `daily-driver-account`

Only for the real copy, "Meguri" with the daily-driver ids, which Antoine studies with beside the test copy "Meguri Dev" (root `AGENTS.md`). Only Antoine sets this up; agents never register identifiers or request profiles.

1. In Certificates, Identifiers & Profiles › Identifiers, register the App IDs `com.antoinefink.meguri` and `com.antoinefink.meguri.widget`.
2. Turn on App Groups for both, with the group `group.com.antoinefink.meguri` (register it if it is missing). On the Mac the routes sign the team-prefixed `DNQ34W43XX.com.antoinefink.meguri`, which needs no registration.
3. In Profiles, create development profiles. For each one, select the Apple Development certificate installed on this Mac, and the device it is for:
   - for the Mac: one **macOS App Development** profile for `com.antoinefink.meguri`, with this Mac;
   - for the iPhone: one **iOS App Development** profile for `com.antoinefink.meguri` and one for `com.antoinefink.meguri.widget`, with the iPhone.

   Both devices are already registered from earlier development builds; add one under Devices with its UDID if not. Then download the profiles: Xcode › Settings › Accounts › your team › Download Manual Profiles. The routes use these portal profiles. They can't use Xcode-managed ones, because they sign manually.

   Don't build Release in Xcode to get them: Xcode would leave a registered daily-driver app in its DerivedData. The routes sign manually with the profile they select and never request provisioning updates.

   The Mac widget is expected to sign without a profile, as in the personal preview. That is unverified until the first run; if the build asks for one, the widget needs a macOS profile and a route change.

The real copy signs no iCloud or push grants. Preserve App Groups, private
app Keychain, certificate, expiry, identity and device authorization. Profiles
with unused iCloud/push authorization remain usable; their presence does not
permit those grants in the signed product.

Doctor's informational `daily-driver` line (it never changes doctor's status) reads the installed profiles (`tools/scripts/daily-driver-profiles.py`). It reports whether one authorizes the real copy on this Mac: identity, App Group, Keychain group, expiry and this Mac. It also reports the iOS app and widget states. It lists this step while any of the three (Mac app, iOS app, iOS widget) is missing or incomplete. It cannot see the developer portal, nor whether the profile's certificate is on this Mac; the build checks that.

Building and installing the real copy, at Antoine's request only. The coordinator installs (standing decision 2026-11-21); nothing launches it:
- **Mac:** `MEGURI_APP_SIGNING=team tools/mg build-app --platform mac --real-copy --owner-requested --install`.
  - Quit Meguri first. Rename an earlier dev install at `/Applications/Meguri.app` to `Meguri Dev.app` first, or the install refuses.
  - It packs `build/app/real-copy/Meguri.zip` and unregisters and deletes the built app. It then verifies the unpacked app in a staging folder, unregisters an earlier real copy and moves it to the Trash (its data stays), and puts the new one in place. Without `--install` it stops at the zip.
  - It then registers `/Applications/Meguri.app` and checks that LaunchServices lists it and no staging path. If that check fails, the app is installed and the failure names the commands that repair or check the registration.
  - The read-only clean-up checks are in `details.real_copy_cleanup_checks`.
  - Don't use Meguri while it builds: until the route forgets it, the built copy is a registered app with the real bundle id, container and Keychain group.
- **iPhone:** `tools/mg run --platform iphone --physical-device <id> --real-copy --owner-requested`.
  - Quit Meguri on the phone first.
  - Unlock the phone and keep it unlocked; Developer Mode must be on (`device-developer-mode`).
  - It checks the app and widget profiles for that phone, then Developer Mode, pairing and lock (read-only), builds the Release, verifies it, and installs it with `devicectl` (a private install witness). It never launches it, and leaves no build product behind.
  - Installing over an earlier real copy keeps its data.

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
