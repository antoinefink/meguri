# Real-copy build route and literal window routes: review log, 2026-11-21

Branch `unit/real-copy-route` (PR #68), infra session `s-20261121T125158Z-305b98c9`. Follow-up to #58, asked for by the coordinator:
- the owner-requested `build-app --real-copy` route;
- the inherited literal-route failure from #50;
- the one-time step `daily-driver-account` and its doctor line.

## Independent review

Reviewer: an independent Claude subagent (Opus), reading the diff and the surrounding tools. It found no defect in the safety properties:
- the route runs only with the acknowledgement and team signing;
- it never passes provisioning flags;
- the profile preflight comes before any build;
- the exit cleanup runs on success, failure and SIGTERM.

Fixed:
- the install step could merge the zip into an existing `/Applications/Meguri.app` (the doc and the clean-up checks now say to move it aside first);
- setup step 4 suggested a Release build in Xcode, which leaves a registered daily-driver app (now portal profiles that include this Mac);
- an unreadable LaunchServices dump passed the forget check;
- exit-path cleanup failures went unreported, and a killed run's leftover was not removed before the preflight;
- doctor kept listing the setup step even with everything done on the account side, because push is still open;
- the doctor line was untested and non-hermetic;
- the preflight accepted cases the product check rejects (`ProvisionsAllDevices`, container supersets, an empty UDID);
- a failure message suggested running the owner-only route again;
- `MG_LSREGISTER` could be overridden from the environment;
- the allowlist regex accepted `-o`;
- the doc text and the widget leftover in the clean-up checks.

A possible problem the reviewer flagged: the window routes now enter `xcode-check`'s hidden launch probe, and their opener calls `openSettings()`, which could bring the app to the front. Fixed by skipping the window when the launch is accessory (background). A drive still opens it; the VM drive shows the Settings window.

Accepted as a note: while the route runs, the built copy is briefly a registered app with the real identity. The setup doc says not to use Meguri during the build.

CodeRabbit raised four comments. The symlink guard and doctor's device coverage were fixed in `2b250b953`; the other two were already fixed in `07ed2833a`.

## Verification

- `selftest tools/tests/real_copy.bats`: 10/10.
- `selftest --filter doctor tools/tests/mg_selftest.bats`: 18/18.
- Real route: `--dry-run` passes. Refusals were checked on this Mac. The settings predicate passes against the real resolved settings. The profile preflight reports no Mac profile here, so the route was not run for real and no daily-driver product was built.
- `xcode-check --settings-only`: 106/106. The literal-route check lists the four window routes.
- `drive` in the VM: each window route launched and showed the Settings window.
- `mac-hosted-test RouteRegistryTests/theStandardRegistryHasTheSampleGalleryPerfShellAndSpikeRoutes`: pass. The parameterized `eachRouteHasItsPresentation` cannot be selected by the focused job, so it was not run alone.
- `lint-ui`, `static-checks --base origin/master` and `similarity` pass.
- Not run: `xcode-check`'s hidden launch phase, because it comes only with the full run, which builds Mac Release.
