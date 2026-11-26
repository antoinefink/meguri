# Native backup and restore verification

Settings can create a device backup, edit the automatic interval and retention counts, and restore an older collection on iPhone, iPad and Mac. Device archives omit media. Restore keeps the local media and creates a safety copy before replacing the collection. A Meguri Sync account can also open its hosted backups in the browser. [Delivery plan](../../plans/2026-11-26-0817-feat-native-backups-plan.md) · [Native screenshot gallery](https://github.com/antoinefink/meguri/pull/230)

## Functional evidence

The following receipts use disposable Meguri Dev collections or synthetic fixtures. Each identifier names a local `build/mg/<job>/<run>/result.json`.

| Scope | Verified result | Receipt |
|---|---|---|
| Backup, scheduler and restore lifecycle | 38 tests passed across six Swift suites on clean `9f5e7cbfa`. Coverage includes cold recovery, cancellation, lifecycle races, retention and sync publication. | `swift-test/20261126T165621Z-20752` |
| Cancellation before restore activation | Explicit Cancel, backgrounding and shutdown retained the synthetic deck and reattached the real offline provider. | `swift-test/20261126T093355Z-64340` |
| Core and native bridge | Four bound-provider restore tests passed, with 79 passing in the surrounding selection. The native bridge and instrumentation selection passed 25 tests. | `core-test/20261126T081912Z-10883`, `core-test/20261126T082001Z-13461`, `swift-test/20261126T090437Z-84979` |
| Retention writes | 13 retention and 19 maintenance FFI tests passed. Single-field changes preserve concurrent updates, unknown members and legacy values. | `core-test/20261126T090027Z-72015`, `core-test/20261126T085906Z-69143` |
| Static checks | Six checks passed, including provenance trailers, recorded sessions and the retry policy. | `static-checks/20261126T171426Z-88943` |

Independent review identified a cancellation path that could leave sync detached and a retention write that could overwrite another device’s edits. Restore now completes reattachment on an uncancelled task. Retention updates read the other settings inside the write transaction. The cancellation and retention tests exercise both fixes.

## Native journeys

Each journey creates a backup, edits a real card, selects an older archive, cancels the first confirmation, restores the selected archive, then recovers the preceding card from its safety copy and checks it after relaunch.

| Platform | Result |
|---|---|
| Mac | The recovery journey passed in `ui-test/20261126T150328Z-14756`. It supplied all three Mac screenshots. The separate 26-row history check passed in `ui-test/20261126T170904Z-67401`. It scrolled to the oldest archive, read its visible date, selected the row, checked the exact restore confirmation, cancelled, and closed the 560 × 262-point window with Command-W. |
| iPhone | The journey, the one-line interval and edge taps on all four number fields passed in `ui-test/20261126T152517Z-88187`. All four rows measured 52 points. The three screenshots matched the approved uploads byte for byte. |
| iPad | The portrait journey passed in `ui-test/20261126T133303Z-95586` and supplied the three approved screenshots. The landscape journey and both Retry actions passed in `ui-test/20261126T153250Z-1971`. In `ui-test/20261126T171422Z-88171`, the interval, value and unit appeared on one line. Each of the four rows measured 52 points, each editable field measured 127 × 44 points, and tapping the edge of each field changed its own value. |

Antoine approved the nine screenshots of Settings, backup history and restore confirmation on November 26. They are available in the [pull request gallery](https://github.com/antoinefink/meguri/pull/230).

## Screenshots and accessibility

The production picker uses a native table in a separate Mac window and a selectable sheet on touch. Ordinary rows show date and size. At accessibility sizes, the date and archive details appear above the size. VoiceOver includes the exact filename. The system confirmation names the selected archive and explains what will happen to the current collection and its media.

`snapshots/20261126T163226Z-93019` rendered all 175 registered backup images. Six matched and 169 were recorded. Independent review accepted 163 changed images. Six Mac error captures showed the divider cutting through Try Again. After the layout fix, `snapshots/20261126T170220Z-47107` recorded all six again. Independent review confirmed the entire button is visible in each image, with 59 points of clear space above the footer divider. All 175 baselines passed independent image review. After the Mac correction, `snapshots/20261126T171741Z-12094` matched all 175 images without recording new baselines. It also captured 23 material probes. The other 490 variants require a separate device session. Three auxiliary Mac tests failed in the same run: account setup, native WebKit replay accessibility, and sentence-mining fixed ports. The same fixed-port test passed in isolation on both the default branch and the feature branch (`mac-hosted-test/20261126T130204Z-35001`, `mac-hosted-test/20261126T173801Z-18038`).

`snapshots/20261126T173626Z-7493` verified 58 iPhone and iPad screenshots on `110739a2c`. It updated 29 folder errors and 14 selection checkmarks. The other 15 images matched. The final comparison on `6df03b7e7` matched all 175 images in `snapshots/20261126T180102Z-75624`: 70 iPhone, 75 iPad, and 30 Mac. It used the same app source as the complete Mac audit.

The iPhone Retry test passed after the compact sheet correction in `ui-test/20261126T162211Z-58202`. It checked both button edges and the minimum 44-point target in the error and empty states.

The iPhone and iPad accessibility runs exercised ten states in light and dark appearance at two text sizes. iPad reported 532 findings, including 269 without an element, label or frame (`a11y/20261126T142952Z-44400`). iPhone reported 423 findings, including 96 without attribution (`a11y/20261126T154257Z-31929`). The audit remains failed. The [accessibility follow-up](https://github.com/antoinefink/meguri/issues/242) tracks these reports and links the diagnostic change.

The Mac audit completed all ten states in light and dark appearance in `a11y/20261126T174459Z-34192`. It verified the exact restore error heading and instruction, their visible frames, and the enabled Retry control. The audit failed with 37 contrast findings and 58 target-size reports. The existing native-window rule excluded 235 other findings. The measured native Mac controls included 24-point buttons and 18-point number fields against the project’s 28-point target. The light screenshots measured 6.33:1 for the server help and 11.60:1 for the selected date. The [Mac accessibility follow-up](https://github.com/antoinefink/meguri/issues/244) tracks the other appearances, confirmation text, and native target sizes.

The final design review, `critic/20261126T181407Z-5994`, reported zero high and five medium findings. Three concerned the native loading indicator. The iPad form capture shows the detail view at full width. The regular-width iPad gallery shows the app with its sidebar. The area above the empty sheet in the cited iPad image is a uniform gray.

The [integration register](../verification-status.md) retains the failed full-project gate. The latest complete gate is `gate/20261116T005859Z-94440`. A later attempt stopped during the Swift suite. The focused results above cover the backup subsystem.
