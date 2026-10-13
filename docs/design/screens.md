# Screens

<!-- Generated from screen-inventory.yaml by `tools/mg design-docs --write`. Do not edit. -->

The readable view of `screen-inventory.yaml`, the only screen inventory (R2001).
Status (sketch, built, verified, approved) lives only in `screen-status.yaml`.
States come from the five-state vocabulary: empty, typical, overflow, error, loading.
Signature screens use the full snapshot covering set and get a variant pick (R1972, R1988);
the others use the pairwise set (U252).

40 screens, 8 signature.

## shell

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| App shell | `shell.root` | U150 | iphone, ipad, mac | typical, loading, error | pairwise set | `sidebar`, `tab-bar`, `toolbar` |
| Keyboard shortcuts help | `shell.keyboardShortcuts` | U158 | ipad, mac | typical, overflow | pairwise set | `sheet-form` |

- `shell.root`: Root layout with the sidebar on iPad and Mac and the tab bar on iPhone, plus the status overlay.
- `shell.keyboardShortcuts`: The list of keyboard shortcuts for the current context.

## onboarding

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Welcome | `onboarding.welcome` | U166 | iphone, ipad, mac | typical, error | pairwise set | `onboarding` |

- `onboarding.welcome`: First launch, opening or creating a collection and the sync choice.

## home

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Home and deck list (signature) | `home.deckList` | U152 | iphone, ipad, mac | empty, typical, overflow, error, loading | full covering set | `list-counts`, `sidebar`, `content-unavailable` |
| Deck page (signature) | `home.deckPage` | U155, U228 | iphone, ipad, mac | empty, typical, overflow, error, loading | full covering set | `list-counts`, `charts` |
| Go to Deck | `home.goToDeck` | U156 | iphone, ipad, mac | empty, typical, overflow | pairwise set | `search-tokens`, `sheet-form` |
| Session summary | `home.sessionSummary` | U157 | iphone, ipad, mac | empty, typical, overflow | pairwise set | `sheet-form`, `charts` |
| FSRS optimization sheet | `home.fsrsSheet` | U51 | iphone, ipad, mac | typical, error, loading | pairwise set | `sheet-form` |

- `home.deckList`: The deck tree with New, Learn and Due counts, the studied-today line and the sync button.
- `home.deckPage`: One deck's counts, study button, finished state and deck statistics.
- `home.goToDeck`: Filter decks by name and jump to one.
- `home.sessionSummary`: End-of-session totals, time studied and the cards missed.
- `home.fsrsSheet`: The FSRS-only notice and parameter optimization status for a collection.

## review

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Review, question side (signature) | `review.question` | U175, U177, U180, U181 | iphone, ipad, mac | typical, overflow, error, loading | full covering set | `progress-header`, `floating-controls`, `toolbar` |
| Review, answer side (signature) | `review.answer` | U175, U178, U179 | iphone, ipad, mac | typical, overflow, error | full covering set | `floating-controls`, `progress-header` |
| Review menu | `review.menu` | U182 | iphone, ipad, mac | typical | pairwise set | `toolbar`, `toast` |
| Set Due Date | `review.setDueDate` | U183 | iphone, ipad, mac | typical, error | pairwise set | `sheet-form` |
| Forget card | `review.forget` | U183 | iphone, ipad, mac | typical | pairwise set | `sheet-form`, `confirmation-dialog` |
| Edit current card | `review.editCurrent` | U184 | iphone, ipad, mac | typical, overflow, error | pairwise set | `editor`, `sheet-form` |
| Congratulations | `review.congratulations` | U186 | iphone, ipad, mac | empty, typical | pairwise set | `content-unavailable` |

- `review.question`: The card's question side with the progress bar, the reveal control and the review menu.
- `review.answer`: The answer side with the Again, Hard, Good and Easy buttons and their intervals, and undo.
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
| Statistics (signature) | `stats.overview` | U225, U228, U230, U231 | iphone, ipad, mac | empty, typical, overflow, error, loading | full covering set | `charts`, `calendar-grid` |
| Heatmap settings | `stats.heatmapSettings` | U229 | iphone, ipad, mac | typical | pairwise set | `grouped-form`, `calendar-grid` |

- `stats.overview`: Today, the heatmap and streaks, and the graph cards for a deck, the collection or a search.
- `stats.heatmapSettings`: Heatmap theme, mode and limits.

## deckOptions

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Deck options (signature) | `deckOptions.options` | U232, U233, U234 | iphone, ipad, mac | typical, overflow, error, loading | full covering set | `grouped-form` |

- `deckOptions.options`: A deck's preset with daily limits, desired retention and the advanced sections.

## settings

| Screen | Id | Units | Platforms | States | Variants | References |
|---|---|---|---|---|---|---|
| Settings | `settings.root` | U160 | iphone, ipad, mac | typical | pairwise set | `grouped-form`, `sidebar` |
| Sync account and status | `settings.sync` | U161 | iphone, ipad, mac | empty, typical, error, loading | pairwise set | `grouped-form`, `confirmation-dialog` |
| Import and export | `settings.importExport` | U162 | iphone, ipad, mac | typical, error, loading | pairwise set | `grouped-form`, `sheet-form` |
| Backups | `settings.backups` | U163 | iphone, ipad, mac | empty, typical, overflow, error, loading | pairwise set | `list-counts`, `confirmation-dialog` |
| Maintenance | `settings.maintenance` | U163 | iphone, ipad, mac | typical, error, loading | pairwise set | `grouped-form` |
| Diagnostics | `settings.diagnostics` | U51, U153 | iphone, ipad, mac | typical, error | pairwise set | `grouped-form` |
| Integrations | `settings.integrations` | U144, U164 | mac | typical, error | pairwise set | `grouped-form` |
| Migaku profiles | `settings.migakuProfiles` | U145 | mac | empty, typical, overflow, error | pairwise set | `table`, `sheet-form` |
| Recently received | `settings.recentlyReceived` | U145 | mac | empty, typical, overflow | pairwise set | `table` |

- `settings.root`: The settings pages and their preferences.
- `settings.sync`: Log in, sync status and the one-way sync choice.
- `settings.importExport`: Import packages and export decks or the collection.
- `settings.backups`: The backup list, creating a backup and restoring one.
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
