# Real copy: development push, pinned profiles, iPhone and Mac install: review log, 2026-11-21

Branch `unit/real-copy-dev-push` (PR #72), infra session `s-20261121T152217Z-240485d1`. Follow-up to #68.
- **Push:** the coordinator decided, under Antoine's delegation, that the owner-requested real copy signs development push.
- **Install:** Antoine asked for the real copy on his iPhone, and for the coordinator to install it on both devices.

Both are recorded in `standing-decisions.md`.

## Independent reviews

Two independent Claude subagents (Opus), one per round. Each read the diff and the surrounding tools, without running builds.

**First round** (development push):
- **High:** automatic signing would never use portal profiles without provisioning updates. Fixed: the routes sign manually with the UUID the preflight selects, and the checks require that UUID.
- **Medium, fixed:**
  - the setup step left out this Mac's certificate and device registration;
  - the decision lacked its authority and a revisit trigger (now cited; `cloud_build_configuration.py` refuses the exception while production readiness is `YES`).
- **Low, fixed:**
  - push is now compared exactly in the preflight;
  - the iPhone note (superseded by the iPhone route);
  - a minimal, explicit list of the steps.

**Second round** (iPhone route, pinning, `--install`):
- **High:** a trailing `real-copy` word after `--personal-preview` selected the real-copy mode without the acknowledgement. Fixed: the mode is passed explicitly; a test covers the trailing word.
- **Medium, fixed:**
  - Xcode-managed profiles are now refused by the preflight;
  - `--install` unregisters the earlier copy before the Trash, unpacks and verifies in a staging folder, and rolls back;
  - the `tools/AGENTS.md` text is corrected;
  - a shell-level success path for the phone route is added (the seal and install steps are fakeable, and unit-tested in Python);
  - the dead jq is removed.
- **Low, fixed:**
  - the device checks now match the personal preview's (exceptions, debugger key, team, identity, widget App Group);
  - the dead branch is removed;
  - the `--install` refusals now happen before building;
  - the phone products are forgotten on every exit;
  - the texts depend on `--install`;
  - any `pgrep` error refuses, and the Trash name is checked first;
  - the `AGENTS.md` wording;
  - the setup doc: iCloud container registration and the unverified Mac widget signing;
  - doctor covers the iOS app and widget.
- **Unverified until the first real run:**
  - the Mac widget signing automatically without a profile;
  - whether a portal macOS profile grants the team-prefixed App Group (the preflight shows `app-group` if not).

CodeRabbit raised two comments on the first round. Both were superseded by the iPhone decision and answered.

The personal preview's identity check is now fakeable. That fixes 5 `physical_device_ui.bats` cases broken since #58.

## Verification

- **Self-tests:**
  - `real_copy.bats` and `cloud_build_configuration.bats`: 17/17;
  - `physical_device_ui.bats`, which includes `physical_drive_test.py` with `RealCopyDeviceTests`: all pass;
  - doctor tests: 19/19.
- **Real settings resolution, without building:**
  - the Mac and iPhone real-copy settings pass the route's checks, with Manual signing and the given UUIDs;
  - Release without the override is rejected by the route;
  - default signing for every other build is unchanged;
  - `xcode-check --settings-only`: 106/106.
- `lint-ui`, `static-checks --base origin/master` and `similarity` pass.
- **Not run:** either real route. Nothing was built, installed or started.
