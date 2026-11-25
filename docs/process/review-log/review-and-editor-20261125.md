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

The Mac VM screenshot of the sparse note exposed a second issue. The field editor displayed Front correctly while the preview showed the third field. The renderer indexed note values by the field's stored number. Preview and Review now share the positional lookup. Independent review `20261125T214218Z-27801` checked the storage, rendering and cache boundaries. All ten rendering tests passed in `core-test/20261125T214539Z-31618`, including persisted dense and sparse normal and cloze notes.

The focused Swift model and input suites passed in `swift-test/20261125T212656Z-3343`. Add, Edit and Browse passed the sparse editor page test in `swift-test/20261125T212946Z-7607`. Focused Continue and the Done keys passed in `swift-test/20261125T213346Z-16842`. The iOS simulator Debug build with nonrepeating R passed in `build-app/20261125T213217Z-12598`.

The VM verified Space, Return and keypad Enter returning to the original deck in `ui-test/20261125T213904Z-23095`. In `ui-test/20261125T214419Z-29340`, Browse rendered the correct Front and retained the new Field 12 value after opening a different note. In `ui-test/20261125T214746Z-33359`, native R queued the question and answer audio in both appearances, a held R did not enqueue again, and typed R stayed in the field. The full-window light and dark captures show card text passing between the opaque rating buttons.

Independent re-review `20261125T213313Z-16038` verified the original three focus and routing findings in source. Native testing subsequently found that Add observed the presentation flag only when its hosting controller was created. The hosted view now observes the flag throughout the window lifetime.

The VM created and studied a card from an empty deck in `ui-test/20261125T220323Z-49704`. The generated card displayed the exact typed front and back. The Add and Add Another, dirty discard, clean close, and selected-deck re-entry cases passed in `ui-test/20261125T215448Z-41365`. The default last-window termination and dirty-Quit case passed in `ui-test/20261125T220145Z-47394`.

Independent review `20261125T215311Z-40487` checked the hosted editor window lifetime. Run `ui-test/20261125T221229Z-57762` showed that a Dock reopen left only Add visible. Independent review `20261125T221403Z-61919` checked the narrow change to reopen the workspace when Add is its only visible window. The native File menu now routes Close Window to the key window through AppKit, including the existing dirty-edit guard.

Pixel comparisons from `ui-test/20261125T214746Z-33359` found the same contents inside all four Mac rating buttons at two scroll positions in each appearance. Pixels in the underlying card text changed in each pair. The pull request holds the native captures.
