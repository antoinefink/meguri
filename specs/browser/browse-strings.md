# Browse strings

Spec: browser/browse-strings
Requirements: R1198, R1199, R1200; supports R108, R899, R1099, R1118, R1119, R1120, R1121, R1122, R1123, R1124, R1125, R1126, R1127, R1128, R1129, R1130, R1133, R1173, R1182, R1206, R1248, R1249, R1253, R1256, R1259, R1268, R1423, R1426, R1428, R1430, R1448
Anki: 26.09.3 for W2 to W4's labels (the English labels of Anki desktop's Find and Replace dialog, Reset dialog and Grade Now buttons); none for W2.1, W5 and W6
Scenarios: none (UI text; W8 lists the tests)

The places where Browse (U210 to U223) shows Anki's exact English wording, and where each such string is listed. U219 and U220 take the strings of W2 to W4 only from this file (KTD1); the strings of W5 come only from the files W5 names. Everything else Browse shows is Meguri's own wording (W2.1, W6, R1959).

## W1. Keys, arguments and plurals

- Every string of W2 to W4 is an entry of the Browse String Catalog (the Browse feature's String Catalog) with the stable key given here. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped; a changed meaning gets a new key.
- No entry here has a plural form, and only `browse.findReplace.error.invalidPattern` (W2.1) has an argument. The labels of W2 to W4 are shown as given, with no trailing period, no colon and no markup; W2.1's texts are whole sentences.

## W2. Find and Replace sheet (R1200)

| Key | English | Used for |
|---|---|---|
| `browse.findReplace.in.allFields` | All fields | the first entry of the In menu, the target `browser/find-replace` names All Fields (F1, F2) |
| `browse.findReplace.in.tags` | Tags | the second entry of the In menu; the entries after it are field names, shown as stored (F3) |
| `browse.findReplace.selectedNotesOnly` | Selected notes only | the checkbox that limits the run to the selected notes (F2, F8) |
| `browse.findReplace.ignoreCase` | Ignore case | the checkbox that makes the match case-insensitive (F4) |
| `browse.findReplace.regex` | Treat input as regular expression | the checkbox that reads the Find text as a regular expression (F4) |

- The checkboxes appear in this order, after the In menu. Their states, defaults and what is remembered are F2's.
- The first In entry is R1200's text, "All fields". `browser/find-replace` writes "All Fields" as its name for that target; the menu shows this key's value. Whether the entry takes title case as a macOS menu item is an open question for Antoine (W7); until a requirement or design-contract change says otherwise, R1200's text stands.
- The sheet's title, the labels of the Find, Replace With and In controls, its buttons and the live matching count are Meguri's wording (W6), and so are W2.1's errors. Anki's control labels carry bold markup and a colon; none of that text is used.

### W2.1 Find and Replace errors (R1206)

Meguri's wording, with no Anki text behind it (Anki shows the regex crate's message alone, in a warning). The sheet shows one of these inline while the user types (`browser/find-replace` F10), and the same text when Apply is refused (F2):

| Key | English | Shown for |
|---|---|---|
| `browse.findReplace.error.invalidPattern` | `Not a valid regular expression: %@` | F4 step 5's `InvalidRegex` when its message has a line that starts with `error: ` (a syntax error, or a pattern over the nesting limit) |
| `browse.findReplace.error.patternTooLarge` | This find text is too large to search with. Make it shorter. | F4 step 5's `InvalidRegex` when its message has no such line (the compiled-size limit, in either mode) |
| `browse.findReplace.error.tagSeparator` | Tags cannot contain spaces. Remove the spaces from Replace With. | F7 step 1's `InvalidInput`, with In set to Tags |

- The argument `%@` of `invalidPattern` is the reason, read from the message as `search/text-matching` T16 reads `{reason}`: the part after `error: ` on the last line that starts with `error: ` (`unclosed group`). It is the crate's English text, shown as it is in every language and never a catalog entry. Example: regex mode, Find `(`: "Not a valid regular expression: unclosed group".
- The find text is not repeated in the message: the field shows it, and F10's range marks the offending part when there is one.
- U220 shows these texts. `browser/find-replace` F7 step 1 calls the separator refusal's wording U219's; the sheet is U220's, and its wording is the row above.

## W3. Reset sheet (R1198)

| Key | English | Starts from |
|---|---|---|
| `browse.reset.restorePosition` | Restore original position where possible | `restorePositionBrowser`, default true (`browser/config-keys` K6.6) |
| `browse.reset.resetCounts` | Reset repetition and lapse counts | `resetCountsBrowser`, default false |

- The two toggles of Browse's Reset sheet (U219), in this order, for the two choices of `sched/manual-ops#M6.1`; the operation stores both values in its step (`browser/bulk-actions` B10, M6.4).
- The review screen's Forget dialog (R386) shows the same English, under its own keys `review.forget.restorePosition` and `review.forget.resetCounts` in `review/reviewer-strings` S14. The two catalogs keep separate keys: Browse never uses the `review.forget.*` keys, and review never uses these.
- The sheet's title, its buttons and its result message are Meguri's wording (W6).

## W4. Grade Now sheet (R1199)

| Key | English | Rating |
|---|---|---|
| `browse.gradeNow.again` | Again | 1 |
| `browse.gradeNow.hard` | Hard | 2 |
| `browse.gradeNow.good` | Good | 3 |
| `browse.gradeNow.easy` | Easy | 4 |

- The four buttons of Browse's Grade Now sheet (U219), always in this order, in one row; keys 1 to 4 always work, and so do the user's own answer keys when they differ (`browser/bulk-actions` B10).
- The review screen's answer buttons show the same rating names (R351), under their own keys `review.answer.again` to `review.answer.easy` in `review/reviewer-strings` S13. The two catalogs keep separate keys: Browse never uses the `review.answer.*` keys, and review never uses these.
- The sheet's title, its Cancel button, any help tag that names a button's key, and its result message are Meguri's wording (W6).

## W5. Strings other files list

Browse shows these Anki strings, each listed in the file named here and taken only from there:

| What | Requirements | Listed in |
|---|---|---|
| The 19 column labels, cards mode and notes mode, and the order of the header menu sorted by them | R1118 to R1130, R1133 | `browser/columns` C2 |
| Cell text: `New #` and a position, `(filtered)`, `(new)`, `(learning)`, a wrapped Due, a filtered card's home deck in parentheses, a notes-mode deck count | R1122 to R1125, R1268 | `browser/columns` C14 |
| Undo and redo titles and step names ("Undo Change Deck") | R1182, R108 | `review/reviewer-strings` S5; the step's `conformance/undo-ops` V4 id per action is chosen by `browser/bulk-actions` B16 and `browser/find-replace` F9 |
| The seven default flag names, shown when `flagLabels` has no name for a flag | R899, R1099, R1173 | `tags/flags-and-marked` F1 |
| Card Info row labels, history labels and kinds, curve labels and ranges, actions and value patterns | R1248, R1249, R1253, R1256, R1259, R1423, R1426, R1428, R1430, R1448 | `stats/card-info` C12 |

- Undo and redo in Browse use S5 as it stands, with no Browse keys of their own: the Edit menu's Undo and Redo items read `review.undo.named` and `review.redo.named` with the step's `undo.step.<id>` name, and `review.undo.plain` and `review.redo.plain`, disabled, when there is no step; the action name given to the system undo manager is the `undo.step.<id>` text alone. The touch toolbar's undo button (R1182) has the accessibility label `review.undo.named` with the step's name, or `review.undo.plain` when there is no step. The Undo button of a result toast (R108) is `review.undo.plain`.
- Search error messages (R1108) and the deck deletion toast (R66) are listed in `search/errors` and `decks/decks` D8; both are Meguri's wording.

## W6. Meguri's wording

Every other text of Browse is Meguri's own wording, or is fixed by the requirement that quotes it: where a requirement quotes a text, the requirement fixes it, even where it is word for word Anki's English. The unit that shows a text writes it. This covers:

- the search field: token menus, the "or" connector, the clear button, Syntax mode, the syntax reference (its headings and the sentences beside its examples; the example texts are Meguri's too, fixed in `search/syntax-reference` Y2 to Y9 and taken only from there), suggestions and their descriptions, the placeholder, the inline error label (R1086 to R1108);
- result and selection counts, empty states, the multi-selection summary and the "Results changed" indicator (R1110, R1111, R1221, R1792);
- the filter sidebar: the section names, which R1157 quotes (Saved Searches, Today, Flags, Card State, Decks, Note Types, Tags; also Anki's), the Today items, and every context menu and management action. R899 quotes "Restore default name", the flag action that removes a flag's name; the same open title-case question as W2's applies to it (W7), and until it is settled R899's text stands. R1159 names the Today items differently from Anki ("Due today" where Anki says "Due"), so those are not Anki's labels (R1157 to R1180; behaviour: `browser/sidebar`);
- menu and command names, shortcuts' menu items and the row context menu (R1186, R1225 to R1231; the commands and their keys: `browser/command-map`);
- bulk result messages: "12 cards suspended", "N notes deleted" and the rest (R1176, R1184, R1191, R1212), written by U218 and U219. The Find and Replace and Edit Field results, which R1207 and R1211 quote as "N of M notes updated" and "N notes updated" (Anki's English too, which ends the second with a period), are U220's, since U220's sheets start them; `browser/find-replace` F9's plural rule applies, and its "U219's" is read as U220's. `browser/bulk-actions` B16 and `browser/find-replace` F9 give Anki's messages for comparison only;
- the Change Deck chooser, the Change Note Type mapping, Add Tags and Remove Tags, the Set Due Date dialog (prompt, hint, validation messages), Edit Field, and the export sheet (R1187, R1189, R1190, R1196, R1208 to R1210, R1213);
- column tooltips (R1132; `browser/columns` C2 keys them);
- Preview and the inspector: Show both sides, tab names, the Current and Previous switch of Card Info (R1216 to R1222, R1263; behaviour: `browser/preview`, `browser/inspector`);
- the iPhone layout: quick-filter chips, status chips ("Now", which R1235 quotes and is also Anki's, and "+N more tags"), toolbars, Select mode and swipe actions (R1234 to R1239);
- Settings › Browse labels (R1267) and every accessibility label (R1240).

## W7. Differences from Anki, and open questions

| Difference | Why | Where |
|---|---|---|
| The In menu's first entry is "All fields" where Anki writes "All Fields" | R1200 quotes it so | W2 |
| No bold markup or colon on any label; the control labels are Meguri's | native form controls | W2 |
| An invalid pattern or tag replacement shows inline as Meguri's sentence, before Apply too | R1206 | W2.1 |

Open question for Antoine: macOS writes menu and pop-up menu items in title case. Should the In entry be "All Fields" and the flag action "Restore Default Name", as `browser/find-replace` F2, `tags/flags-and-marked` F4 and U217's approach write them? That needs R1200 and R899 corrected, or a design-contract exception like `docs/design/native-interface-contract.md` N11.7's title-case labels; this file follows the requirements until then. The same question covers the Browse empty state's action, which R1110 quotes as "Clear search" and the `browse.browser` contract writes "Clear Search" (`browser/entry-points` E7); R1110's text stands until it is settled.

## W8. Tests

The oracle runs the collection library, not Anki's UI, so the text itself is checked by Meguri's tests, each in a file its unit already lists:

| Test file (unit) | Covers |
|---|---|
| `FindReplaceModelTests` (U220) | W2: the five keys, the In menu's order (All fields, Tags, then field names) and the checkbox order; W2.1: each error for its refusal, the reason read from a multi-line message, `(` giving "Not a valid regular expression: unclosed group", the size text in literal mode |
| `ResetSheetTests` (U219) | W3: both keys, their order and their starting values |
| `SelectionLaunchersTests` (U219, which lists no Grade Now test file of its own) | W4: the four keys in order on the Grade Now sheet |

Every key must exist in the Browse String Catalog with exactly this English value (U167's string lint). This check belongs to the unit that adds the catalog, runs in the `tools/mg lint-ui` job with U167's `lint-strings`, and starts when the catalog exists (`text/interface-text` I3).
