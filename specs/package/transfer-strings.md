# Import and export strings

Spec: package/transfer-strings
Requirements: R1601, R1618, R1625; supports R156, R191
Anki: 26.09.3 for S2 and S3 (the English labels of Anki desktop's deck package import options and export options); none for S4
Scenarios: none (UI text; S5 lists the tests)

The places where U162's import and export sheets (`package/transfer-sheets`) show Anki's exact English wording. U162 takes these strings only from this file (KTD1); `text/interface-text` I3 lists it. Everything else those sheets show is Meguri's own wording (S4, R1959).

## S1. Keys and form

- Every string of S2 and S3 is an entry of the Settings feature's String Catalog under the key given here, with a developer comment. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped; a changed meaning gets a new key.
- No entry has an argument or a plural form. Each is shown as given: no trailing period, no colon, no markup.

## S2. Deck package import options (R1601)

| Key | English | Option (`package/apkg-import#I5`) |
|---|---|---|
| `import.apkg.withScheduling` | Import any learning progress | `withScheduling` |
| `import.apkg.withDeckConfigs` | Import any deck presets | `withDeckConfigs` |
| `import.apkg.mergeNotetypes` | Merge note types | `mergeNotetypes` |
| `import.apkg.updateNotes` | Update notes | `updateNotes` |
| `import.apkg.updateNotetypes` | Update note types | `updateNotetypes` |
| `import.apkg.update.ifNewer` | If newer | the value `"IfNewer"` of either update option |
| `import.apkg.update.always` | Always | `"Always"` |
| `import.apkg.update.never` | Never | `"Never"` |

- The order on the sheet is `package/transfer-sheets#X2.2`'s. The three value labels are shared by both update options, in the order If newer, Always, Never.
- R1601 writes the values in lower case ("if newer / always / never") in running text; the labels are Anki's, as above.

## S3. Export options (R1618, R1625)

| Key | English | Option (`package/apkg-export#A2`, A14) |
|---|---|---|
| `export.includeScheduling` | Include scheduling information | deck package only |
| `export.includeDeckPresets` | Include deck presets | deck package only |
| `export.includeMedia` | Include media | both formats |
| `export.legacySupport` | Support older Anki versions | both formats (`package/legacy-export`) |

- The order on the sheet is `package/transfer-sheets#X4.3`'s.
- Anki 26.9.3's label for the last option ends with " (slower/larger files)". Meguri shows R1618's text without it. Whether the sheet adds its own footnote about speed and size is Meguri's choice.
- "Anki" in `export.legacySupport` names the other app as a plain compatibility fact (R156). U167's `lint-strings` allowlist must hold this key (`text/interface-text` I7).

## S4. Meguri's own wording

Not Anki's text, and never copied from it:

- the sheets' titles, the update options' group title if the screen contract groups them (Anki: "Updates"), the help texts of every option (Anki's are not used), the format names (Anki's contain the product name, R156), the whole-collection and selection entries of the scope list, the preview facts of `package/apkg-import#I4`;
- the result lines and the reasons in the result table (`package/transfer-sheets#X2.5`), the collection package confirmation and its message (X3), the export result (X4.4), every error message (`package/apkg-import#I16`, `package/apkg-export#A12`, `package/container#P13`), the intake messages of `package/file-intake` K3, K4 and K6, and Welcome's question of K6.1.
- Changed 2026-11-23 (deck package import): the merge preview's title `Import “<file name>”?`, its rows, its backup line and its Import button (`package/transfer-sheets#X2.3`); the sync paragraph, its unreachable variant and the "Import and Upload" button of a structural import with Meguri Sync (X2.3, `sync/meguri-provider#MP7.1`), whose English is given there; the message and offer of each refusal of X2.6's table, `TooLarge` (each `what`), `DeckTooDeep`, `InsufficientSpace`, `Stale`, `Busy`, `SafetyBackupFailed` and `SessionClosed` included; the other engine refusal, the error after the commit and the `read_options` failure of the same table; the text shown for a log entry without fields and the line counting them, and the line counting skipped media files (X2.5). None is Anki's text.

## S5. Tests

- A catalog test: each S2 and S3 key exists with the English value given and a developer comment, and the import and export sheets use these keys for these labels.
- The snapshot tests of U162 show the S2 labels in X2.2's order and the S3 labels in X4.3's order for each format.
