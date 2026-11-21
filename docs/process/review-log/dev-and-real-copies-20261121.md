# Meguri Dev name and blue icon: review log, 2026-11-21

Branch `unit/dev-and-real-copies` (PR #58), infra session `s-20261121T103919Z-b6732c84`.
Scope:
- Development-identity builds (`.dev`: Debug, Profile, the personal preview) show "Meguri Dev" with a blue icon; daily-driver builds stay "Meguri" with the red icon.
- A generator for the blue icon, and identity checks in `tools/mg xcode-check`.
- Agent rules for the real copy and the test copy.

The PR description holds the readiness report for installing the real copy. Antoine asked for the blue icon on 2026-11-21.

Commit `01750eb3b` holds all the review fixes: configuration, checks, rules and icon. Its message describes only the icon part. Files left staged by earlier rejected attempts went into it, and it was pushed before anyone saw this.

## Independent review

Reviewer: an independent Claude subagent (Opus). It received the diff and the brief, and was asked to look for any state the two identities share and to check the icon at every size. It read the branch, rendered both icon documents with `ictool` (iOS and macOS, every rendition, 16–1024 px) and inspected the built products read-only.

- **High: widget settings.** The identity settings check failed on every run: the scheme's resolved settings list only the app target. Fixed: the widget is resolved on its own with `-target MeguriWidget` and its name is checked for every configuration and the personal preview.
- **High: name in Finder and the Dock.** The Mac shows a localized display name only when Info.plist's `CFBundleDisplayName` matches the bundle's file name. `NSFileManager` reported "Meguri" for the Debug build. Fixed:
  - on the Mac, Info.plist keeps `Meguri`, with `LSHasLocalizedDisplayName`;
  - the localized strings carry "Meguri Dev";
  - `NSFileManager` now reports "Meguri Dev", and `app_identity_check.py` checks that name.
- **Medium: clear and tinted styles.** Both icons look the same there, because the system draws them monochrome. This is left for Antoine (a non-colour cue would change shapes, which the brief excluded). It is recorded in the README and the PR.
- **Medium: shared state.** Shared: the `meguri://` scheme, log subsystems and the Meguri Sync account a test copy is signed in to. Recorded in `app/AGENTS.md` and the PR's readiness report. No code change.
- **Low-medium: mottled pith.** The pith turned mottled. Fixed: the band's upper hue limit drops for bright pixels.
- **Low: fade through purple.** The fade passed through purple. Fixed: a partial change now goes through grey, and a test sweeps the hue circle.
- **Low: gamut clipping.** Clipping flattens chroma detail. Left as is: lightness texture is kept; noted in the PR.
- **Low, fixed:**
  - colour strings outside sRGB or in unknown spaces;
  - `check` blind to generator changes (the record now holds the generator's digest);
  - the BMP format guard;
  - stray icon files of the other identity;
  - the personal-preview product was never identity-checked (`build-app` now checks it);
  - `--settings-only` deleted products and overstated "builds nothing";
  - unquoted exclusion paths;
  - doc wording.
- **Low, accepted:** the default `xcode-check` checks the daily-driver iOS product's identity only through `TestHooksTests` and the signing matrix.

CodeRabbit raised five comments (the `xcode-check` exception, quoting twice, the core build in `--settings-only`, the fade). Each is answered on the PR: four were fixed, and for the core build the documentation was corrected.

## Verification

Runs on `01750eb3b` (run ids under `build/mg/<job>/`):

- **Mac Debug:** `build-app/20261121T113832Z-94444`. The Mac Debug build passed with 0 warnings, and its product shows "Meguri Dev" through `NSFileManager`, with the blue `AppIcon-Dev` only.
- **iOS simulator:** `build-app/20261121T113142Z-58363` (Debug, dev identity) and `20261121T113240Z-65034` (Release, daily identity, red `AppIcon` only) both pass `app_identity_check.py product`.
- **Personal preview:** `build-app/20261121T113603Z-79020`, team-signed, passes its validation and the new identity check.
- **Settings:** `xcode-check/20261121T114053Z-9777` (`--settings-only`) passed 106/106. Every configuration and the personal preview resolve to the right name, icon, exclusions and widget name. No daily-driver Mac product was built.
- **VM capture:** `drive/20261121T114402Z-29608` then `screenshot/20261121T114649Z-37162` on pool iPhone 5. The Home Screen shows the blue icon labelled "Meguri Dev".
- **Other checks:** `lint-ui/20261121T114725Z-41407`, `image-tools-selftest/20261121T114729Z-42850` (218 passed), `similarity/20261121T114751Z-44308` (55 files, 0 flagged). `static-checks` ran on the branch before the review fixes and passed. Private scans passed in the pre-commit hook, after rewording flagged comment lines.

Not run: the full `xcode-check` (it builds Mac Release and AppStore) and `--cloud-signing-matrix` (it builds team products). No `tools/mg` job captures the Mac Dock or app switcher in the VM.
