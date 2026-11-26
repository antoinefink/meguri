# Backups production rendering evidence handoff

List-read failure repair (native verification pending): the restore error fixture
now models a list-load error separately from operation failures. With no entries,
the sheet shows “Unable to Load Backups”, its typed explanation and one Try Again
action in a native ScrollView. The explanation keeps its full vertical size to
address the reported compact-touch accessibility truncation. A successful empty
read retains “No Backups Yet”. Restore failures, activation and safety disclosures
remain available, including when a subsequent list refresh fails. The explanation
uses `settings.backups.listFailure`; existing action and entry identifiers remain.
The runner did not build, test or render this correction. Recheck the focused
BackupCoordinatorTests, BackupsAccessibilityTests and affected native matrix cells.

Scope: `settings.backups` (U163a), `package/backups` B2/B6/B7,
`package/backup-triggers` T8, R1964, R1970–R1972, R1977–R1982.

The task is to create a device backup or choose an eligible dated restore copy.
Creation, progress, eligibility and the media consequences remain visible;
replacement consequences appear in the native destructive confirmation.
The existing production grouped Form, selectable List and confirmation retain
platform defaults. There is no new visual design or custom control.

`BackupsView` and `BackupRestoreSheet` map production state to the internal,
immutable `BackupsPresentation`. `BackupsContent` and `BackupRestoreContent`
render both production and synthetic fixtures. Only production wrappers refresh,
open folders, export, cancel work or restore. Production model setters remain
encapsulated. Fixtures construct no host, collection, core, file or service;
a fixture OpenURLAction consumes even the browser link without opening it.
`BackupCoordinator`, `BackupTrigger`, `ReviewCollection` and the functional
journey test are untouched.

## Matrix and routes

`BackupsPreview` registers the existing inventory ID `settings.backups` and all
five inventory states with `.pairwise` on iPhone, iPad and Mac. This minimal
matrix spans the screen's two production surfaces:

| State | Production surface and synthetic content |
|---|---|
| empty | Restore sheet, empty list with Try Again and disabled Restore |
| typical | Settings, successful receipt, limits, browser action and media facts |
| overflow | Restore sheet, older verified server copy selected; unverified and rejected entries ineligible |
| error | Restore sheet, failed folder read with actionable retry and disabled Restore |
| loading | Settings, backup creation progress and safe cancellation |

Additional literal routes cover **each** surface in **each** state:
`settings/backups/{empty,typical,overflow,error,loading}` (`screen`) and
`settings/backups/restore/{empty,typical,overflow,error,loading}` (`sheet`).
Settings overflow shows full UInt32 limits; restore typical selects the newest
eligible copy; restore loading shows the named read progress indicator.
Route inventory and presentation-style expectations include all ten routes.
`BackupsSnapshotTests` asserts every state has exactly the platform's pairwise
cells, retaining the matrix's existing device-session deferrals.

## Accessibility evidence producer

`BackupsAccessibilityTests` uses the existing `AccessibilityAudit` and
`TouchTargets` utilities on all three platforms. It covers ten surface/state
combinations, light/dark, and default/AX5 text on touch (Mac's native default
text size). It scrolls native Forms/Lists to audit fields and rows, asserts
labels and enabled states, full UInt32 values, dates/sizes/exact filenames,
selected older copy, verified-copy and ineligibility labels, device/server media
facts and the actual native restore confirmation's selected date and provider
publication disclosure. Its restore callback is inert.

Stable screen anchors: `settings.backups.content` and
`settings.backups.restoreContent`. Existing actions/list/entry IDs are preserved.
Added targets: `settings.backups.limit.{interval,daily,weekly,monthly}`,
`settings.backups.retry`, `.loading`, `.progress`, `.mediaExplanation`,
`.serverExplanation`, `.safetyCopy` (all relative to `settings.backups`).
Selected rows expose the selected accessibility trait. Progress has a named label
and an “In progress” value.

Each platform's MG run exports `a11y-screen.settings.backups.json` in XCTest
attachments and lists it in `screens.json`, with ten state results and audit and
target findings. These are evidence producers, **not executed passes**.

## Exact integrator commands

Run from the reviewed task checkout on the configured Mac builder. MG keeps
app-hosted rendering and all UI work inside the test VM. Execute sequentially:

```sh
tools/mg mac-hosted-test --suite BackupsSnapshotTests/everyInventoryStateUsesThePairwiseMatrixOnEveryPlatform
tools/mg mac-hosted-test --suite RouteRegistryTests/theStandardRegistryHasTheSampleGalleryPerfShellAndSpikeRoutes
tools/mg snapshots --platform iphone --filter 'settings.backups/*' --record 'settings.backups/*'
tools/mg snapshots --platform ipad --filter 'settings.backups/*' --record 'settings.backups/*'
tools/mg snapshots --platform mac --filter 'settings.backups/*' --record 'settings.backups/*'
tools/mg a11y --platform iphone --suite BackupsAccessibilityTests
tools/mg a11y --platform ipad --suite BackupsAccessibilityTests
tools/mg a11y --platform mac --suite BackupsAccessibilityTests
```

After inspecting recorded images, verify without recording:

```sh
tools/mg snapshots --platform iphone,ipad,mac --filter 'settings.backups/*'
```

Baselines: `app/Snapshots/<model>-<OS build>/settings.backups/<state>/<cell>.png`.
MG run output retains new images, diffs, registry/state coverage and result
receipts. The ordinary snapshot bundle also executes route presentation checks.
Do not treat record mode, a filtered pass or fixture appearance as native design
acceptance or a functional restore pass.

## Verification status

Filtered-runner source implementation and consistency review only. No build,
MG verification, native capture, accessibility report or baseline was generated
here, as requested. The commands above, independent source/privacy review,
exact public-file hash admissions, provenance/similarity checks and native image
review remain with the integrator. Existing failures in
`docs/process/verification-status.md` remain unchanged and must be reported on
the PR; this patch claims none repaired.

The separately owned true functional journey must independently capture actual
app windows for the PR. Synthetic matrix/audit fixtures do not replace it.
