# Round 2 integration (2026-11-20)

Session `s-20261120T080628Z-cd0d7aaa` (fixer), branch `unit/round2-integration`, started at `0dc7d0b50` (`unit/round2-tests-fix`).

## Merges

- `origin/master` at `c2b24f49c` (`6c362bc13`). One conflict: `docs/design/screens.md`, regenerated with `tools/mg design-docs --write` (55 screens).
- `unit/round2-meguri-setup-fix` at `4f0a81cc8` (`6a1f64619`). Two conflicts:
  - `SyncKinds.swift`: the cancelled-setup key constant of the tests branch, with the two cases of the setup branch.
  - `SyncSettingsSections.swift`: the sign-in draft, the Account page and the status row layout of the tests branch, with the setup branch's core-decided keep-collection sheet, its setup start and its Try Again. The two-step state (`choosing`, `close`) is gone. Status actions outside Settings open the Sync section, which holds the one presentation site of each sheet.
- `core/` equals the setup branch's `core/` plus the tests branch's `cloud_media_cleanup.rs`.

## Fixes after the merge

- `0f46d9971`: a cancelled Meguri Sync round counts as waiting for study preparation.
- `002f13af3`: keep-collection sheet title wraps on iPhone and iPad; Mac focus starts on Cancel; popover footer spacing; Troubleshooting footer for a connected device; the Mac journey follows the single confirmation.
- `44ea0d5de`: merge review findings (below).
- `669a0b18e`: two clippy findings in the setup branch's code, with no behaviour change.

## Troubleshooting on a connected device

`sync/status-strings` T19.6 and `sync/meguri-provider` MP6.7 list both replace entries once the service is set up. The core refuses a bootstrap over a ready binding (`MeguriBootstrapError::AlreadyBound`, asserted in `meguri-host/tests/provider_bootstrap.rs`), and no spec clause says how a proved baseline is given up at the user's request. No core transition was added. Both entries stay disabled while connected, with the footer `sync.trouble.replace.connected.meguri` (proposed for T19.6, not yet approved). They run while setup is required, through MP4's preview and its one destructive confirmation. For the owner: decide the mechanism in MP6.7, or change T19.6.

## Merge review

Independent read-only reviewer (subagent, did not write the merge), three-way diffs against both branches. Verdict: no blocker; no path replaces or uploads a collection without the required confirmation; the setup branch's safety checks and the tests branch's migration and hold rules are unchanged; one presentation site per sheet; every string key is in the catalog.

Findings fixed in `44ea0d5de`:

1. The Troubleshooting footer said more than is true. It now reads "Not available while this device is connected to Meguri Sync."
2. The flag that continues a Sign In into setup was view state, lost when a layout change rebuilt the section. It lives in `MeguriSignInDraft`.
3. A keep-collection request could stay set after the settling step, or into a later Sign In. Both now clear it.
4. The Mac default focus was set on the form, outside the toolbar's Cancel. It is set on the navigation stack.

Left as notes: the Mac ordering of the request and the Settings window is the tests branch's; no UI test taps a Troubleshooting replace entry.

## Checks

Recorded in the pull request.

## AI review of the pull request

Three passes by the AI reviewer on pull request 7. Each finding was checked against the code and the specs. Fixes: `dd12d0457`, `864523aef`, `443e7fbc7`, `980eb2886`. The answers, with the reason for each finding left unchanged, are in the pull request.

`980eb2886` also builds the three texts that the specs marked "not built" (`sync.keep.downloadOnly.message.meguri`, `sync.needsYou.meguri.finishSetup`, `sync.needsYou.meguri.localUnusable`) and removes the "is replaced" line from the upload-only sheet.

Valid findings deferred to follow-up work. None can replace or upload a collection without its confirmation.

- iCloud, whose work is paused: `SyncCoordinator.swift` (the update hold clears when the cancel dialog opens, not at the confirmed cancel; a stored Sync Automatically off against the core's committed value, two places; the recovery-read failure flag), `SyncKinds.swift` (note type name placeholder), `CloudConnectionView.swift` (Resume and Set Up Again inside the resume sheet).
- Meguri Sync: `meguri-ffi/src/provider_bootstrap.rs` (admission refresh in the settlement worker; recoverable through the existing refresh), `ProviderBootstrapService.swift` (interval names; the active slot during a decision read), `Settings.swift` (sign-in draft above the layout switch), `one_way.rs:375`.
- Tests and previews: `SyncSettingsPreview.swift`, `SyncStringCatalogTests.swift`, `MeguriSetupSequenceTests.swift`.
