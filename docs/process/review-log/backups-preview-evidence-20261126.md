# Native backup and restore verification

Settings can create a device backup, edit the automatic interval and retention counts, and restore an older collection on iPhone, iPad and Mac. Device archives omit media. Restore keeps the local media and creates a safety copy before replacing the collection. A Meguri Sync account can also open its hosted backups in the browser. [Delivery plan](../../plans/2026-11-26-0817-feat-native-backups-plan.md) · [Native screenshot gallery](https://github.com/antoinefink/meguri/pull/230)

## Functional evidence

The following receipts use disposable Meguri Dev collections or synthetic fixtures. Each identifier names a local `build/mg/<job>/<run>/result.json`.

| Scope | Verified result | Receipt |
|---|---|---|
| Backup, scheduler and restore lifecycle | 37 tests passed across six Swift suites on clean `44305c7be`. Coverage includes cold recovery, cancellation, lifecycle races, retention and sync publication. | `swift-test/20261126T114421Z-27219` |
| Cancellation before restore activation | Explicit Cancel, backgrounding and shutdown retained the synthetic deck and reattached the real offline provider. | `swift-test/20261126T093355Z-64340` |
| Core and native bridge | Four bound-provider restore tests passed, with 79 passing in the surrounding selection. The native bridge and instrumentation selection passed 25 tests. | `core-test/20261126T081912Z-10883`, `core-test/20261126T082001Z-13461`, `swift-test/20261126T090437Z-84979` |
| Retention writes | 13 retention and 19 maintenance FFI tests passed. Single-field changes preserve concurrent updates, unknown members and legacy values. | `core-test/20261126T090027Z-72015`, `core-test/20261126T085906Z-69143` |
| Static checks | Six checks passed, including provenance trailers, recorded sessions and the retry policy. | `static-checks/20261126T114419Z-26798` |

Independent review identified a cancellation path that could leave sync detached and a retention write that could overwrite another device’s edits. Restore now completes reattachment on an uncancelled task. Retention updates read the other settings inside the write transaction. The cancellation and retention tests exercise both fixes.

## Native journeys

Each journey creates a backup, edits a real card, selects an older archive, cancels the first confirmation, restores the selected archive, then recovers the preceding card from its safety copy and checks it after relaunch.

| Platform | Result |
|---|---|
| Mac | `ui-test/20261126T093850Z-76474` passed on a dirty checkout at `d322d24e5`. The compact window implementation was committed in `cc1202cea`. The capture includes the native titlebar and all three table rows in a 560 × 262 point window. |
| iPhone | `ui-test/20261126T104835Z-79610` passed on clean `8d39140df`. All three rows fit, the title is visible, and the selected date matches the confirmation. |
| iPad | `ui-test/20261126T115746Z-69296` passed on clean `034acbe90`. The native three-row sheet uses 52-point rows and a 268-point height. All rows fit inside its bounds. |

The gallery contains three original app-window PNGs per platform. Its nine uploaded files matched the original captures byte for byte. Updated Settings colors on touch require fresh captures.

## Screenshots and accessibility

The production picker uses a native table in a separate Mac window and a selectable sheet on touch. Ordinary rows show date and size. At accessibility sizes, the date and archive details appear above the size. VoiceOver includes the exact filename. The system confirmation names the selected archive and explains what will happen to the current collection and its media.

`snapshots/20261126T120907Z-12962` matched 156 images and recorded 19 replacements. All 175 supported cells were represented, and 490 device-session variants were deferred. Independent review confirmed that standard and AX5 renders displayed different text sizes, that AM and PM fit without splitting, and that all changed PNGs contained only synthetic UI. The top-level snapshot receipt failed on three companion Mac tests covering native replay accessibility, normal account settings and fixed ports. All 109 route presentation cases passed in `snapshots/20261126T110252Z-47032`.


The [integration register](../verification-status.md) retains the failed full-project gate. The latest complete gate is `gate/20261116T005859Z-94440`. A later attempt stopped during the Swift suite. The focused results above cover the backup subsystem.

The independent checks reviewer approved the complete public verification register at SHA-256 `078604c0ea8b0dd0f821da5407fbea6b5325ba03f83cc809ac57d7e7b8a2f88e` and the complete native interface contract at `7156ed8b110f1d63d92d86e839d846af11cac625ac0752815694acda38a70d57`. The three reviewed snapshot manifests have SHA-256 `f5d117ee410ed0bf037ead622ba6828c7c455214f0861041e31fc469d7843f09`, `1aaf9cfc8294e163785d86e0725ab3fa9340be5c6f55f67a23a1804b53734428` and `97ee84edf2d06a49780b060c404c121423c00d32f2fe727cca7fccce66b202d9`.
