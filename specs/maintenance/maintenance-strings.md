# Maintenance strings

Spec: maintenance/maintenance-strings
Requirements: R152, R285 (the three commands), R1649 (the Check Media actions), R1653 (the Empty Cards option); supports R952, R1639, R1640, R1652, R1660, R1661
Anki: 26.09.3 for S2 to S4's labels (the English of Anki desktop's Tools menu items, its Check Media report buttons and its Empty Cards option) and S4's starting value; none for S5
Scenarios: none (UI text; S6 lists the tests)

The places where U163's Maintenance page, its reports and the Tools menu (U159) show Anki's exact English wording. U163 and U159 take these strings only from this file (KTD1); `text/interface-text` I3 lists it. Everything else they show is Meguri's own wording (S5, R1959). The runs behind them are `maintenance/check-database`, `maintenance/check-media` and `notes/empty-cards`.

## S1. Keys and form

- Every string of S2 to S4 is an entry of the Settings feature's String Catalog under the key given here, with a developer comment; the Tools menu items use the same keys. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped.
- No entry has an argument or a plural form. Each is shown as given, with no trailing period and no colon.

## S2. Commands (R152, R285)

| Key | English | Where |
|---|---|---|
| `maintenance.checkDatabase` | Check Database | the Maintenance page's button, the Tools menu item, the damaged-collection screen's remedy (R169) |
| `maintenance.checkMedia` | Check Media | the Maintenance page's button, the Tools menu item |
| `maintenance.emptyCards` | Empty Cards… | the Maintenance page's button, the Tools menu item |

- Anki 26.9.3's Tools menu shows "Check Database" and "Check Media", which start at once there, and "Empty Cards..." with three periods, which opens a report first. Meguri writes the ellipsis as the character U+2026, as Apple's menus do and as R285 writes it.
- The labels are R285's, which match Anki's. In Meguri, Check Media starts at once and Empty Cards… opens its report first, so their forms follow Apple's rule (an ellipsis when the command needs more from the user before it runs). Check Database is the exception to check: Anki starts it at once, but Meguri asks first (`maintenance/check-database#C2`, R2020), and under Apple's convention a command that opens a confirmation usually ends in an ellipsis (Finder's Empty Trash…). R285's "Check Database" stands until Antoine decides (S7); a change would also reach the damaged-collection screen's remedy (`shell.damagedCollection`) and R285.
- Anki's menu items carry keyboard accelerator marks; Meguri's do not (macOS has none).

## S3. Check Media report actions (R1649)

| Key | English | Action (`maintenance/check-media`) | Offered when (C12) |
|---|---|---|---|
| `checkMedia.tagMissing` | Tag Missing | C13 | `missing_notes` is not empty |
| `checkMedia.deleteUnused` | Delete Unused | C14 | `unused` is not empty |
| `checkMedia.emptyTrash` | Empty Trash | C17 | the trash is not empty |
| `checkMedia.restoreTrash` | Restore Trash | C16 | the trash is not empty |

- The first three are Anki's labels. The fourth is R1649's: Anki 26.9.3 labels this button "Restore Deleted", the name `maintenance/check-media` C16 uses for the action. Until Antoine decides otherwise (S7), the button shows R1649's "Restore Trash".
- Delete Unused and Empty Trash open a confirmation in Meguri (C14, C17), as in Anki; their labels have no ellipsis in Anki or in R1649. The same ellipsis question as S2's Check Database applies to them (S7).
- The tag that Tag Missing adds is `missing-media` (C12, C13). It is an interface value, never a catalog entry, and it is the same in every language. Anki's tag comes from its translation files, whose English is `missing-media`; Meguri never translates it (a note tagged in another language's Anki then differs from Meguri's tag).
- Anki also offers "Render LaTeX" (not offered: C8, C20) and a button that opens the media folder. Meguri offers no button to open the media folder (R1649 lists none).

## S4. Empty Cards option (R1653)

| Key | English | Starts from |
|---|---|---|
| `emptyCards.keepNotes` | Keep notes with no valid cards | on, every time the report opens |

- The toggle of the Empty Cards report, passed as the delete's "keep notes" input (`notes/empty-cards#E4`).
- Anki 26.9.3's report opens with it on each time and never remembers a change; Meguri does the same: the value is not stored anywhere, so a report always starts on.
- When the scan finds nothing (`notes/empty-cards#E2`'s empty result), Anki shows a short message and no report, so the toggle never shows. Meguri does the same; the message is Meguri's (S5).

## S5. Meguri's own wording

Not Anki's text, and never copied from it:

- the Maintenance page's section titles, status lines (`maintenance/optimize-integrity#M10`), progress labels of every stage (`maintenance/check-database#C15`, Anki's stage labels are not used), Cancel;
- the Check Database report: one line per count and the no-problems line (`maintenance/check-database#C12`: Anki's messages are never shown or stored), and the corrupt result that points to backups (R1640);
- the Check Media report: its counts, section headers and per-file lines (`maintenance/check-media#C12`), and the confirmations of Delete Unused (with its count, C14) and Empty Trash (with its count and size, C17), and every result message;
- the Empty Cards report: the note type headings, the per-note lines, the all-empty marking (`notes/empty-cards#E2`), the delete button, the no-empty-cards message and the result message (E4).

## S6. Tests

- A catalog test: each S2 to S4 key exists with the English value given and a developer comment; the Tools menu items and the Maintenance page buttons use the S2 keys.
- U163's tests: the Check Media report offers each S3 action exactly when C12's condition holds; the Empty Cards report opens with S4's toggle on after a run in which it was turned off; a scan with no empty cards shows the message and no report.

## S7. Open questions

- R1649's "Restore Trash" or Anki's "Restore Deleted" for the Check Media restore button. Both describe C16; the Product Contract's wording stands until Antoine chooses.
- "Check Database" or "Check Database…", and "Delete Unused" and "Empty Trash" with or without "…": each opens a confirmation in Meguri, which Apple's convention marks with an ellipsis, while R285, R1649 and Anki write none. The Product Contract's labels stand until Antoine chooses; the catalog value changes, not the key.
