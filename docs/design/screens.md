# Screens

<!-- Generated from screen-inventory.yaml by `tools/mg design-docs --write`. Do not edit. -->

The readable view of `screen-inventory.yaml`, the only screen inventory (R2001).
Status (sketch, built, verified, approved) lives only in `screen-status.yaml`.
States come from the five-state vocabulary: empty, typical, overflow, error, loading.
Signature screens use the full snapshot covering set and get a variant pick (R1972, R1988);
the others use the pairwise set (U252).

44 screens, 8 signature.

## shell

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| App shell | `shell.root` | U150 | iphone, ipad, mac | typical, loading, error | pairwise set | `sidebar`, `tab-bar`, `toolbar` |
| Keyboard shortcuts help | `shell.keyboardShortcuts` | U158 | ipad, mac | typical, overflow | pairwise set | `sheet-form` |
| Damaged collection | `shell.damagedCollection` | U153, U163a, U251 | iphone, ipad, mac | empty, typical, error, loading | pairwise set | `content-unavailable`, `confirmation-dialog` |

- `shell.root`: Root layout with the sidebar on iPad and Mac and the tab bar on iPhone, plus the status overlay.
- `shell.keyboardShortcuts`: The list of keyboard shortcuts for the current context.
- `shell.damagedCollection`: Shown in Failed{Corrupt} (engine/lifecycle L1, maintenance/optimize-integrity M7) in place of the whole root content: no sidebar, tab bar, deck list or review. A native content-unavailable view with the exclamationmark.triangle symbol, the title "This Collection Is Damaged" and one line, "Meguri stopped saving changes and sending anything to iCloud so the damage can't spread.", then the remedies as buttons in M7.1's order, each shown only while M7.1 offers it: "Check Database" (U163 once U128 lands), "Restore from Backup…" (when the backups listing holds a backup or server copy; opens settings.backups' restore sheet with the newest restorable backup preselected) and "Replace with iCloud Collection…" (only when sync/cloudkit-recovery V19's conditions hold; opens its V14 preview). The first offered remedy has the prominent style, the others the bordered style; "Copy Debug Info" is always there as a plain button below them. No remedy downloads from an Anki server. Empty: no remedy is offered, so the line adds "No backup or iCloud copy of this collection is available on this device." and only Copy Debug Info shows. Loading: Check Database's stages or the iCloud entry's progress replace the buttons, with Cancel while the operation says stopping is safe. Error: Check Database or the iCloud entry failing or refusing (Check Database's corrupt verdict, offline, SafetyBackupFailed) shows its reason in one line above the remaining buttons; the screen stays. A restore started here reports its progress and errors in settings.backups' restore sheet. A remedy that succeeds returns to the deck workspace. Every menu command that needs a collection is disabled; Settings, Copy Debug Info and Quit stay available. In Settings every row stored in the collection is disabled; device-local rows, Backups' Restore from Backup… and Show in Finder or Files, and About stay usable.

## onboarding

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Welcome | `onboarding.welcome` | U166 | iphone, ipad, mac | typical, error | pairwise set | `onboarding` |
| Import Collection | `onboarding.collectionImport` | U166 | iphone, ipad, mac | empty, typical, overflow, error, loading | pairwise set | `sheet-form` |

- `onboarding.welcome`: First launch, opening or creating a collection and the sync choice.
- `onboarding.collectionImport`: Choose a latest-format collection-only .colpkg, confirm inspected counts and local replacement safety, and follow cancellable import progress or a typed failure.

## home

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Home and deck list (signature) | `home.deckList` | U152, U228 | iphone, ipad, mac | empty, typical, overflow, error, loading | full covering set | `list-counts`, `sidebar`, `content-unavailable` |
| Deck page (signature) | `home.deckPage` | U155, U228a, U228 | iphone, ipad, mac | empty, typical, overflow, error, loading | full covering set | `list-counts`, `charts` |
| Deck menu | `home.deckMenu` | U152, U156 | iphone, ipad, mac | typical | pairwise set | `list-counts`, `confirmation-dialog` |
| Deck name sheet | `home.deckName` | U156 | iphone, ipad, mac | typical, error | pairwise set | `sheet-form` |
| Go to Deck | `home.goToDeck` | U156 | iphone, ipad, mac | empty, typical, overflow | pairwise set | `search-tokens`, `sheet-form` |
| Session summary | `home.sessionSummary` | U157 | iphone, ipad, mac | empty, typical, overflow | pairwise set | `sheet-form`, `list-counts` |
| FSRS optimization sheet | `home.fsrsSheet` | U51 | iphone, ipad, mac | typical, error, loading | pairwise set | `sheet-form` |

- `home.deckList`: The deck tree as a native list (native-interface-contract N2, N11.5): each row the deck's last name component with the non-zero New, Learning and To Review counts as secondary text, the Mac and iPad sidebar rows with a To Review badge; then the activity section (studied-today line, heatmap, streak stats; U228) in the same list on every layout; and the sync button.
- `home.deckPage`: One deck's labelled New, Learning and To Review counts with the buried "+N" badge (U228a), the Study button, the finished state, and the deck-scoped heatmap, streak stats and Stats action (U228).
- `home.deckMenu`: One menu for a deck, shown as the row's context menu (right-click, long press) and the Mac row's hover ⋯ button, in R47's order with Delete separated and destructive and Unbury only with buried cards; a filtered deck's menu offers only Rename and Delete (R53). On iPhone a row swipe shows Options and More, never Delete (R49). The menu bar's Deck menu is U159's and follows R283. Until U156 lands the menu holds Study and Overview (U152). Delete of a subtree with cards asks first with the deck and card counts (confirmation-dialog); deletes and moves end with an Undo toast.
- `home.deckName`: The sheet for Create Deck, New Subdeck (prefilled "<parent>::", cursor at the end) and Rename (prefilled full path): one name field, Cancel and a confirm button disabled for an empty or blank name. Error: the refused move or name in one line under the field; a final name that differs from the typed one is reported after saving.
- `home.goToDeck`: Filter decks by name and jump to one.
- `home.sessionSummary`: A sheet over the restored deck workspace (window sheet on Mac, form sheet on iPad, sheet on iPhone), shown when R91 says (queue empty, timebox Finish, Back to Decks or Esc after at least one answer). Title "Session Summary"; toolbar Done (confirmation action; Return and Esc too) and Undo (⌘Z; while an answer can be undone). A grouped list: (1) totals: cards studied and time ("42 cards in 12 minutes"), Again, Hard, Good and Easy counts as four labelled values, "Passed" as the percentage of answers that were not Again, and new cards introduced; (2) What's Next: remaining today with a bordered Continue button after an early close (Done keeps Return), or the time until the next learning card, or "Finished for today" with tomorrow's due count; the finished state's limit and buried notes with their actions (R94); (3) the streak line once U228 fills its slot, taking no space before; (4) Missed Cards in first-miss order: question excerpt (two lines at most), deck name as secondary text, the miss count trailing ("2×"), a deleted card labelled "Deleted card"; a row opens the read-only preview once U186 attaches it, and "Browse Missed Cards" sits under the section once U210 attaches it. Empty: no missed cards, the section reads "No missed cards". Overflow: many missed cards scroll inside the sheet; the toolbar stays. The deck page's "Last Session" opens the same sheet read-only: title "Last Session", no Undo, Continue or Unbury. No chart.
- `home.fsrsSheet`: The FSRS-only notice and parameter optimization status for a collection.

## review

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Review, question side (signature) | `review.question` | U175, U177, U180, U181 | iphone, ipad, mac | typical, overflow, error, loading | full covering set | `progress-header`, `floating-controls` |
| Review, answer side (signature) | `review.answer` | U175, U178, U179 | iphone, ipad, mac | typical, overflow, error | full covering set | `floating-controls`, `progress-header` |
| Review menu | `review.menu` | U182 | iphone, ipad, mac | typical | pairwise set | `toolbar`, `toast` |
| Set Due Date | `review.setDueDate` | U183 | iphone, ipad, mac | typical, error | pairwise set | `sheet-form` |
| Forget card | `review.forget` | U183 | iphone, ipad, mac | typical | pairwise set | `sheet-form`, `confirmation-dialog` |
| Edit current card | `review.editCurrent` | U184 | iphone, ipad, mac | typical, overflow, error | pairwise set | `editor`, `sheet-form` |
| Congratulations | `review.congratulations` | U186 | iphone, ipad, mac | empty, typical | pairwise set | `content-unavailable` |

- `review.question`: The card's question side in quiet corners (native-interface-contract N3, N11.1, N11.2): Back to Decks at the upper left, the timer, touch undo and ⋯ and the sync status icon at the upper right, the thin progress bar under them, the card scrolling between them and a reserved bottom action region holding the Show Answer button.
- `review.answer`: The answer side: the same corners and card position as the question side, and in the same bottom region the Again, Hard, Good and Easy buttons in one row, interval as the prominent label and grade name secondary, no key hints (N11.2). iPhone (compact width, any iPad window that narrow): full screen above the hidden tab bar with the status bar visible, the exit an xmark close button named "Back to Decks", undo and ⋯ at the upper right, the bottom region above the home indicator and clear of the fold, buttons at least 44 pt tall with Dynamic Type capped to keep one row (R358), card taps and tap zones as U180. iPad at regular width: the review fills the window with the sidebar hidden for the whole session, the labelled Back to Decks button, and the iPhone touch rules; with a hardware keyboard the shortcuts appear in the menu bar and the ⌘ overlay, not on the buttons. Landscape and short windows keep the corner controls and the one-row bottom region; only the card area shrinks.
- `review.menu`: In-place card actions (bury, suspend, flag, mark, edit) and their toasts.
- `review.setDueDate`: Set the current card's due date from the review screen.
- `review.forget`: Reset the current card to new with its two remembered options.
- `review.editCurrent`: Edit the note of the card under review without leaving the session.
- `review.congratulations`: Nothing left to study today, with what comes next.

## editor

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Add note (signature) | `editor.add` | U190, U192, U193, U195 | iphone, ipad, mac | empty, typical, overflow, error | full covering set | `editor`, `toolbar` |
| Edit note | `editor.edit` | U190, U196 | iphone, ipad, mac | typical, overflow, error | pairwise set | `editor`, `inspector` |

- `editor.add`: Add notes with the note type and deck pickers, fields, formatting, tags and media.
- `editor.edit`: Edit an existing note from the browser or review.

## noteTypes

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Note types | `noteTypes.list` | U200 | iphone, ipad, mac | empty, typical, overflow, error, loading | pairwise set | `list-counts` |
| Card templates | `noteTypes.templates` | U200 | iphone, ipad, mac | typical, overflow, error | pairwise set | `editor`, `inspector` |
| Fields | `noteTypes.fields` | U201 | iphone, ipad, mac | typical, overflow, error | pairwise set | `grouped-form` |
| Template preview | `noteTypes.preview` | U202 | iphone, ipad, mac | typical, error, loading | pairwise set | `inspector` |
| Change Note Type | `noteTypes.changeNoteType` | U203 | iphone, ipad, mac | typical, overflow, error | pairwise set | `sheet-form` |

- `noteTypes.list`: The note types with their note counts and management actions.
- `noteTypes.templates`: Edit front, back and styling templates of a note type.
- `noteTypes.fields`: Add, rename, reorder and configure a note type's fields.
- `noteTypes.preview`: Preview the question and answer a template renders.
- `noteTypes.changeNoteType`: Map fields and templates when moving notes to another note type.

## imageOcclusion

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Image occlusion editor | `imageOcclusion.editor` | U205, U206 | iphone, ipad, mac | empty, typical, overflow, error, loading | pairwise set | `toolbar`, `inspector` |

- `imageOcclusion.editor`: Draw, group and edit masks over an image.

## browse

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Browser (signature) | `browse.browser` | U210, U211, U213, U214, U216, U221, U223 | iphone, ipad, mac | empty, typical, overflow, error, loading | full covering set | `table`, `search-tokens`, `sidebar`, `inspector` |
| Find and Replace | `browse.findReplace` | U220 | iphone, ipad, mac | typical, error | pairwise set | `sheet-form` |
| Edit Field | `browse.editField` | U220 | iphone, ipad, mac | typical, error | pairwise set | `sheet-form` |

- `browse.browser`: Search, sort and act on cards or notes, with the filter sidebar and the inspector.
- `browse.findReplace`: Find and replace text in the selected notes' fields.
- `browse.editField`: Set one field across the selected notes.

## cardInfo

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Card Info | `cardInfo.card` | U222 | iphone, ipad, mac | typical, overflow, error, loading | pairwise set | `inspector`, `charts` |

- `cardInfo.card`: A card's properties and review history.

## stats

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Statistics (signature) | `stats.overview` | U225, U230, U231 | iphone, ipad, mac | empty, typical, overflow, error, loading | full covering set | `charts`, `calendar-grid` |
| Heatmap settings | `stats.heatmapSettings` | U229 | iphone, ipad, mac | typical | pairwise set | `grouped-form`, `calendar-grid` |

- `stats.overview`: The scope and history header and the graph cards in R1357's order (Today, Future Due, Calendar, ... Added) for a deck, the collection or a search. The heatmap and streak stats are not here: they live on Home and the deck page (home.deckList, home.deckPage).
- `stats.heatmapSettings`: The Heatmap settings section: Calendar mode, the Advanced disclosure with the history, forecast and date limits, the deleted-card and manual-reschedule exclusions and the excluded decks (R1339), first day of week, and the device-local display toggles. No colour theme: one native palette (R1315); the synced theme value is kept unchanged.

## deckOptions

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Deck options (signature) | `deckOptions.options` | U232, U233, U234 | iphone, ipad, mac | typical, overflow, error, loading | full covering set | `grouped-form` |

- `deckOptions.options`: A deck's preset with daily limits, desired retention and the advanced sections.

## settings

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Settings | `settings.root` | U160 | iphone, ipad, mac | typical | pairwise set | `grouped-form`, `sidebar` |
| Sync account and status | `settings.sync` | U161 | iphone, ipad, mac | empty, typical, error, loading, overflow | pairwise set | `grouped-form`, `confirmation-dialog` |
| Import and export | `settings.importExport` | U162 | iphone, ipad, mac | typical, error, loading | pairwise set | `grouped-form`, `sheet-form` |
| Backups and Restore | `settings.backups` | U163a | iphone, ipad, mac | empty, typical, overflow, error, loading | pairwise set | `grouped-form`, `sheet-form`, `list-counts`, `confirmation-dialog` |
| Maintenance | `settings.maintenance` | U163 | iphone, ipad, mac | typical, error, loading | pairwise set | `grouped-form` |
| Diagnostics | `settings.diagnostics` | U51, U153 | iphone, ipad, mac | typical, error | pairwise set | `grouped-form` |
| Integrations | `settings.integrations` | U144, U164 | mac | typical, error | pairwise set | `grouped-form` |
| Migaku profiles | `settings.migakuProfiles` | U145 | mac | empty, typical, overflow, error | pairwise set | `table`, `sheet-form` |
| Recently received | `settings.recentlyReceived` | U145 | mac | empty, typical, overflow | pairwise set | `table` |

- `settings.root`: The settings sections and their preferences (native-interface-contract N5, N11.4). Mac: the separate Settings window (⌘,) with a sidebar of sections and a search field; iPad and iPhone: the Settings sidebar section or tab of the main window, sections as rows. Each section is a grouped form.
- `settings.sync`: Explicit Anki collection transfers and separate both-side media consent. Overflow renders media-consent; supplemental settings/anki-transfer routes media-progress, media-partial, media-complete and media-unknown cover Cancel, explicit Resume, long excluded names and unverified exclusions using the production controls with synthetic values and no server work.
- `settings.importExport`: Import packages and export decks or the collection.
- `settings.backups`: The Backups section (Mac: a Settings window sidebar section, plus File › Back Up Now and File › Restore from Backup…; iPad and iPhone: the Backups row of Settings). A grouped form: (1) "Back Up Now" with "Last backup: <date and time>" from the maintenance status (optimize-integrity M10), and the result "Created <file name> (<size>)" in the footer after a run; (2) "Restore from Backup…" and "Show in Finder" (Mac) or "Show in Files" (iPad, iPhone; Documents/Backups); (3) the limits with the Synced badge: "Back up every" N minutes (5–9999), "Daily backups", "Weekly backups", "Monthly backups" (0–9999 each), written only on change (package/backups B6.2). Each field shows the value B6.1 reads, even outside its range; an edit changes only the edited member and passes the other three read values to the B6.2 write unchanged. Footer "Backups don't include media files." and, once U162 attaches it, "Export Collection with Media…". The restore sheet (on the window it was opened from; the main window from the damaged-collection screen): title "Restore from Backup", a list newest first of date and time, size, and "Server copy" for a server copy; an unverified server copy is listed dimmed with "Can't be restored" and cannot be selected; the newest restorable entry is preselected; Cancel and Restore (disabled without a selection). The sheet stays open through the confirmation, the progress and a refusal, and closes on success. Restore asks first (confirmation-dialog): "Replace the current collection with the backup from <date>?", message "The current collection is backed up first. The restored collection stays on this device until you choose what to do with iCloud.", destructive Restore and Cancel. Loading: Back Up Now is a background job (package/backup-triggers T5): a non-modal activity in its row with Cancel while stopping is safe; only Back Up Now and Restore from Backup… are disabled meanwhile, and study and edits go on. The restore runs in the exclusive phase: its progress replaces the sheet's list, with Cancel while stopping is safe, and U153's gate disables mutating entry points app-wide. Error: a Back Up Now failure shows its cause in the section footer. A refused or failed restore shows one line in the sheet naming the cause and saying the current collection is unchanged: a backup that fails its check (the next-newest restorable entry is then preselected), a review session open, another operation running (named), the safety backup failing, not enough space. Empty: no backups yet; Restore from Backup… disabled with the footer "No backups yet". In Failed{Corrupt} Back Up Now and the limits are disabled and Restore stays. Success lands on the deck workspace with the iCloud publication choice that the pending iCloud contract defines.
- `settings.maintenance`: Check Database, Check Media and Empty Cards with their reports.
- `settings.diagnostics`: Diagnostic information and Copy Debug Info.
- `settings.integrations`: The AnkiConnect and Migaku servers, their state and access settings.
- `settings.migakuProfiles`: Routing profiles that map Migaku cards to decks and note types.
- `settings.recentlyReceived`: Notes the local servers created recently, with move actions.

## widget

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Heatmap widget | `widget.heatmap` | U165 | widget | empty, typical | pairwise set | `widget`, `calendar-grid` |

- `widget.heatmap`: The study heatmap and streak in small and medium widgets.
