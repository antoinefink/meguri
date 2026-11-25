# Review controls and Mac note editing

The review footer must keep its existing controls legible over a long card. The finished screen's Space and Return keys should run Done, and R should replay the current card side. On Mac, Add Note should use the same window layout as Edit Note with selectable note type and deck.

## Browse regression

The storage reader sorts fields by stored ordinal and preserves each row's number. The editor rejects any layout whose stored numbers differ from the fields' array positions. This includes imported layouts with gaps, which `notetypes/records` N1 supports. A regression test must edit the later field in such a layout and verify that the earlier field keeps its contents.

## Session record review

An independent reviewer (`metadata_review`) read each complete session record, including its path, and admitted its contents as public operational metadata.

| Session | SHA-256 |
| --- | --- |
| `s-20261125T204705Z-a4cccc69` | `842b74e1b4a9b559907781493b1f5d49df10eb41d49010a4e44b7ad68d5f0362` |
| `s-20261125T204711Z-ce6a10fc` | `a649400bfa0eb037a5f04e6c14bd55afd50bc121872bce1d6aae286d2ffcd291` |
| `s-20261125T205344Z-c62797ef` | `e6cf48aa7ca13848aaebd8cd2249d19c8d323a346ecc74b57d83cc586f886e22` |
| `s-20261125T205411Z-6adbfd92` | `c7326e3710115a489880c52c517c7df55520b83cece19ed42a3dfc7e815c08a4` |
| `s-20261125T211224Z-f0bbe96f` | `7d764e729b819fb4efeeacd3b22f41e0d8185c1ae1c196914401435d4a6f0921` |

The same reviewer read the complete 4,666-byte `core/crates/meguri-testkit/fixtures/recipes/review-editor.yaml` exported by the implementation session. All text, HTML, media declarations and names are authored synthetic fixture data. Admitted SHA-256 `47fca0bec62e69b0a5d8cf51dab5a6c740c8f17923383f130453125ac480195b` matches the committed fixture.

## Independent source review and verification

The independent source review in `20261125T210924Z-86529` found three Mac issues. The fixes admit R when a native review control has focus, preserve the requested deck when New Note brings its window forward, and route Close to the active window. The PR review also found that Browse could report a committed save as failed when retrying an edit after the layout changed. Both save paths now acknowledge the submitted revision when the backend reports that it committed.

Seven storage patch tests passed in `core-test/20261125T210215Z-83032`, including saving a later field, reopening the collection and checking its stored layout. The FFI cloze-position regression passed in `core-test/20261125T210402Z-85179`. The UI fixture regression in `core-test/20261125T212823Z-6275` verified the stored gaps, all 14 field values in their logical order and both cards on the note.
