# Native backup and restore verification

Settings can create a device backup, edit the automatic interval and retention counts, and restore an older collection on iPhone, iPad and Mac. Device archives omit media. Restore keeps the local media and creates a safety copy before replacing the collection. A Meguri Sync account can also open its hosted backups in the browser. [Delivery plan](../../plans/2026-11-26-0817-feat-native-backups-plan.md) · [Native screenshot gallery](https://github.com/antoinefink/meguri/pull/230)

## Functional evidence

The following receipts use disposable Meguri Dev collections or synthetic fixtures. Each identifier names a local `build/mg/<job>/<run>/result.json`.

| Scope | Verified result | Receipt |
|---|---|---|
| Backup, scheduler and restore lifecycle | 37 tests passed across six Swift suites on clean `51fbe5e0b`. Coverage includes cold recovery, cancellation, lifecycle races, retention and sync publication. | `swift-test/20261126T133439Z-7679` |
| Cancellation before restore activation | Explicit Cancel, backgrounding and shutdown retained the synthetic deck and reattached the real offline provider. | `swift-test/20261126T093355Z-64340` |
| Core and native bridge | Four bound-provider restore tests passed, with 79 passing in the surrounding selection. The native bridge and instrumentation selection passed 25 tests. | `core-test/20261126T081912Z-10883`, `core-test/20261126T082001Z-13461`, `swift-test/20261126T090437Z-84979` |
| Retention writes | 13 retention and 19 maintenance FFI tests passed. Single-field changes preserve concurrent updates, unknown members and legacy values. | `core-test/20261126T090027Z-72015`, `core-test/20261126T085906Z-69143` |
| Static checks | Six checks passed, including provenance trailers, recorded sessions and the retry policy. | `static-checks/20261126T133318Z-97884` |

Independent review identified a cancellation path that could leave sync detached and a retention write that could overwrite another device’s edits. Restore now completes reattachment on an uncancelled task. Retention updates read the other settings inside the write transaction. The cancellation and retention tests exercise both fixes.

## Native journeys

Each journey creates a backup, edits a real card, selects an older archive, cancels the first confirmation, restores the selected archive, then recovers the preceding card from its safety copy and checks it after relaunch.

| Platform | Result |
|---|---|
| Mac | `ui-test/20261126T132033Z-28841`, clean `03359cb22`. The recovery journey passed. The separate long-list test scrolled one page and failed before reaching the oldest row. |
| iPhone | `ui-test/20261126T133142Z-89261`, clean `51fbe5e0b`. The recovery journey passed, including Cancel and Restore in the system alert. |
| iPad | `ui-test/20261126T133303Z-95586`, clean `51fbe5e0b`. The recovery journey passed, including the safety copy after relaunch. |

The nine PNG files on the pull request matched these three runs byte for byte.

## Screenshots and accessibility

The production picker uses a native table in a separate Mac window and a selectable sheet on touch. Ordinary rows show date and size. At accessibility sizes, the date and archive details appear above the size. VoiceOver includes the exact filename. The system confirmation names the selected archive and explains what will happen to the current collection and its media.

`snapshots/20261126T120907Z-12962` matched 156 images and recorded 19 replacements. All 175 supported cells were represented, and 490 device-session variants were deferred. Independent review confirmed that standard and AX5 renders displayed different text sizes, that AM and PM fit without splitting, and that all changed PNGs contained only synthetic UI. The top-level snapshot receipt failed on three companion Mac tests covering native replay accessibility, normal account settings and fixed ports. All 109 route presentation cases passed in `snapshots/20261126T110252Z-47032`.


The [integration register](../verification-status.md) retains the failed full-project gate. The latest complete gate is `gate/20261116T005859Z-94440`. A later attempt stopped during the Swift suite. The focused results above cover the backup subsystem.
