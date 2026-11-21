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

### `cloudkit-setup`

Only Antoine configures containers, capabilities, profiles and schema promotion
(R2065). `tools/mg doctor` does not verify this step. Packaging checks are not
evidence of a working account or encrypted schema.

Review the proposed container IDs in `app/Config/Cloud.xcconfig`:
`MEGURI_CLOUD_CONTAINER_development` is `iCloud.com.antoinefink.meguri.dev`;
`MEGURI_CLOUD_CONTAINER_production` is `iCloud.com.antoinefink.meguri`.
Verify their existence during setup and use separate private databases.
Development and test configurations use the isolated container. The Mac
personal preview also requests development explicitly, together with `.dev`;
the suffix by itself supplies no cloud selection or permission. Ordinary
Release and AppStore builds use the production container.

Assign only the isolated development container to development app profiles;
keep daily-driver container access out of those profiles. Enable CloudKit and
push for the app's Mac/iOS identities, with the selected container, CloudKit
service, `Development`/`Production` iCloud environment and
`development`/`production` push environment. Mac uses
`com.apple.developer.aps-environment`; iOS uses `aps-environment`. Preserve
existing App Groups, private app Keychain and sandbox grants. Widgets receive
no CloudKit or push grants. Personal Release.dev uses development-only
container/environment authorization, just like other development/test profiles.
Refresh the appropriate profiles and verify their grants, signer, device
coverage and expiry before signed builds. Ad-hoc Mac previews need none of
these grants and always report schema readiness false.

Create and inspect the encrypted record schema in
[`sync/cloudkit-records` K2–K5](../../specs/sync/cloudkit-records.md): the six
record types use encrypted `e` values and only the specified asset fields;
collection content must not become plaintext custom fields. Verify the isolated
development schema, explicitly promote/verify the daily-driver production schema,
and retain separate environment/container evidence from signed multi-device runs.
Only after each environment's actual identifiers, profile grants and encrypted
schema are verified may its `MEGURI_CLOUD_SCHEMA_CONFIGURED_development` or
`MEGURI_CLOUD_SCHEMA_CONFIGURED_production` change from `NO` to `YES`. Never
enable readiness to satisfy a build check. The packaged
`MeguriCloudContainer`, `MeguriCloudEnvironment` and Boolean
`MeguriCloudSchemaConfigured` must match that verified setup.

The integrator can check packaging with
`tools/mg xcode-check --no-ui --cloud-signing-matrix` once profiles exist. This
adds team Mac/device/simulator builds for every configuration without launching
them or requesting provisioning updates; it retains resolved settings, generated
Info and signed/embedded entitlements under `build/xcode-check/cloud-*`. It
does not certify portal configuration, schema encryption/promotion, push delivery
or signed multi-device sync. Those external checks remain required.


### `cloudkit-test-accounts`

Only Antoine runs `tools/mg cloudkit-account-setup --label acct1` in his own
foreground Terminal, from clean master with no competing Apple or VM job.
Source `.local/activate` first. Use `MEGURI_APP_SIGNING=team`; if the verified
Mac build pins an installed profile with `MEGURI_CLOUD_AGENT_MAC_PROFILE_FILE`,
retain that same setting. Keep the dedicated VM signed in to the intended
isolated development test account.

The setup-only producer verifies the signed product and observes the live
account. At its terminal prompt, Antoine confirms the intended account with
`approve acct1`. A fresh matching observation precedes publication of the
private approved source. Agents never run this command or enter the approval.
No account value or private file content needs to be shared; report only the
setup outcome and run ID. `acct1` is enough for the initial same-account check;
`acct2` is a separate setup for account-isolation checks, after `acct1` exists.

The source belongs to this host user's OS Application Support data. Copying the
repository or VM to another Mac does not publish it on the new host. No source
importer or candidate-file adoption is supported. Missing source input is a
setup requirement, not evidence of a failed CloudKit exchange. Successful setup
supplies no signed-sync or release acceptance.


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

1. In Certificates, Identifiers & Profiles, register the App IDs `com.antoinefink.meguri` and `com.antoinefink.meguri.widget`.
2. Register the App Group `group.com.antoinefink.meguri` and assign it to both App IDs. On the Mac the group is the team-prefixed `DNQ34W43XX.com.antoinefink.meguri`.
3. On `com.antoinefink.meguri`, enable iCloud (CloudKit) with the container `iCloud.com.antoinefink.meguri`, and Push Notifications. The container needs no schema: iCloud Sync stays off in the real copy while `MEGURI_CLOUD_SCHEMA_CONFIGURED_production` is `NO` (`app/Config/Cloud.xcconfig`).
4. Create a macOS App Development profile for each App ID that includes this Mac as a registered device, and an iOS one for the iPhone later. Use the portal, then download them with Xcode › Settings › Accounts › Download Manual Profiles. Don't build Release in Xcode to get them: Xcode would leave a registered daily-driver app in its DerivedData. `tools/mg build-app --real-copy` uses installed profiles only and never requests provisioning updates.

Push needs a decision before the route can pass. Release signs `com.apple.developer.aps-environment` = `production` (KTD22), but Mac development profiles grant push `development` only. So a development-signed real copy is refused at signing even after the steps above. The two ways out:
- a Developer ID certificate and profile (production push), with a signing variant;
- development push for this personal Release.

Either changes the signing policy and needs Antoine's choice. Until then the route stops at its profile check, which names `push-production`.

Doctor's informational `daily-driver` line (it never changes doctor's status) reads the installed profiles (`tools/scripts/daily-driver-profiles.py`). It reports whether one authorizes the real copy on the Mac (identity, App Group, Keychain group, the production container and environment, production push, expiry, this Mac), and the iOS state. It lists this step while the Mac profile is missing or incomplete. When only production push is missing, the step names the push decision instead. It cannot see the developer portal itself.

Building and installing the real copy, at Antoine's request only:
1. Run `MEGURI_APP_SIGNING=team tools/mg build-app --platform mac --real-copy --owner-requested`. It packs `build/app/real-copy/Meguri.zip`, unregisters and deletes the built app, and prints the read-only clean-up checks (`details.real_copy_cleanup_checks`). Don't use Meguri while it runs: until the route forgets it, the built copy is a registered app with the real bundle id, container and Keychain group.
2. Antoine installs it. First quit Meguri and move any existing `/Applications/Meguri.app` aside: rename an earlier dev install to `Meguri Dev.app`, and move an earlier real copy to the Trash. This matters because `ditto` merges into an existing bundle and breaks its signature. Then, from the checkout, run `ditto -x -k build/app/real-copy/Meguri.zip /Applications/`.

**iPhone: a documented option, not built.** `run --physical-device --personal-preview` installs only the dev identity. A real copy on the phone would need:
- the iOS profiles from step 4;
- a `run --platform iphone --physical-device <id> --real-copy --owner-requested` route, mirroring the personal preview: Release with the daily ids, team signing from installed profiles only, a `devicectl` install and its sealed install witness;
- the same push decision.

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
