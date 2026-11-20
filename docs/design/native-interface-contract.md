# Native interface contract

This contract defines Antoine's agreed macOS direction for the current native preview.
Source: Antoine's approved HTML preview and feedback on 2026-10-24.
The HTML specifies hierarchy and behavior. It does not specify native control measurements.

## Design objective and precedence

Meguri should look and behave like an app designed by Apple on each platform.
Use native controls and platform defaults for typography, colors, sizing, spacing and interaction.
Customize only for a concrete functional, compatibility or verified accessibility need.
Keep exceptions minimal and document the need and verification in the active screen contract or change log.
Design tokens alone do not justify overrides. Cosmetic exceptions require Antoine's explicit approval.
Preserve user-authored study-card styling and rendering compatibility.

Antoine approved this direction on 2026-10-25. It takes precedence over R1992, R1994 and KTD20 where they require tokens instead of platform defaults.
Existing token assets and lint checks constrain justified custom values; they do not require replacing native defaults.
Keep accessibility checks and functional color meanings. An accessibility override needs evidence from the affected control and state.
Verify the exact built app visually, including disabled, selected and keyboard-focus states where applicable.
Passing checks alone do not establish native appearance.

## N1. Scope and authority

- Use native SwiftUI controls, semantic styles, SF Symbols and macOS menus.
- Apply the design objective and exception rules above to every screen and shared component.
- Keep Settings category navigation visible without a sidebar-toggle toolbar button. Align row labels and controls vertically; avoid excess row space caused by nested control frames.
- Keep the existing collection, review, import, export and server-transfer behavior functional.
- Apply shared control lessons to iOS and iPadOS while keeping their native touch targets and navigation patterns. Inspect their native renders too. The agreed workspace composition in this contract targets macOS.
- Antoine's newer direction overrides R6, R109 and R110 only for the separate native macOS Settings scene.
- Quiet corners overrides R21's option to reveal the sidebar during review. Restore the deck workspace after review.
- Preserve the underlying navigation and saved-answer semantics of R5, R12, R13 and R22.
- This bounded UI task does not resume the full application goal.
- Build, open and smoke-check this preview now. Broad gates follow visual agreement; do not claim unit acceptance.

## N2. Deck workspace

- Deck selection is the main workspace. Show the real collection's deck tree and available counts.
- The Decks root uses a native list, with ordinary-weight names and secondary count text. Let the system provide row surfaces, focus and hierarchy disclosure. Do not embed the list in another scroll view or draw navigation chevrons. Keep the full study breakdown on the selected deck. This replaces the custom card-like root rows (Antoine, 2026-10-25).
- Make Study the selected deck's primary action. Use `ReviewCollection.startStudy(deckID:)` and its existing availability guard.
- Center the selected deck and empty state in the available detail pane. Overflow content remains scrollable; fixed top padding is not the centering mechanism.
- On Mac, use a content-sized Study button and simple deck-name/due-count sidebar rows. The selected deck shows the three labeled counts without filling the sidebar with that full sentence.
- Mac deck rows use the system sidebar surface and selection. Keep navigation rows to one line; retain the full deck path in accessibility and hover help. This bounded truncation keeps long names from taking over the sidebar. Touch rows retain wrapping.
- Use a native numeric badge for the Mac sidebar count, without a visible “due” suffix. Retain its meaning in the accessibility value. Give the deck summary space with system padding around its count grid and labels (Antoine, 2026-10-25).
- Keep a previous-session summary secondary and aligned with the selected deck. Show it there only when the saved session belongs to that deck; keep collection-wide history available on the Decks root.
- The sidebar shows deck names and counts. With no deck rows, show “No decks”.
- Keep import in File, Collection settings and empty-state onboarding.
- Remove permanent sidebar import actions, routine deck-page import buttons and standalone shortcut links below Study.
- Keep empty-state text brief: identify the missing collection and offer an import action.
- Preserve distinct collection-loading, unavailable, failed, no-file and empty-deck states.
- Do not advertise unavailable Browse, Stats or editing features as working destinations.

Supporting requirements: R15, R30, R31, R35, R40–R42. Show existing features without claiming full requirement coverage.

## N3. Quiet corners review

- Remove the sidebar and content header/title throughout review.
- Keep a visible “Back to Decks” action at the upper left.
- Keep a quiet transfer-state icon at the upper right. Its popover explains the real recorded state. During review it shows only while sync needs the person (N16.2).
- Do not infer remote freshness from a prior transfer, saved credentials or the absence of errors.
- Keep Show Answer and the four rating actions at the bottom of the review area.
- Reserve the action region on both sides so reveal does not move or resize card content.
- Long card content scrolls between the corner controls and the answer region.
- Reveal preserves the reader's scroll position. Never recreate the card pair just to change the surrounding layout.
- Preserve the real rating labels and intervals from `ReviewCoordinator.item.nextStates`.
- Keep waiting, finished, paused/error and transition states usable.
- Keep undo and redo in the native menus and retain their accessibility actions.
- Back to Decks calls the existing finish-review path. Saved answers remain saved; the unrated card remains unrated.
- Preserve review lifecycle, media, timing, visibility and obscuration signals when windows or Settings change focus.

Supporting requirements: R131, R134, R135, R1975, R1976, R1980–R1982.

## N4. Native commands and keyboard ownership

- Put shortcuts beside actual native menu commands. Do not add prominent key badges to review buttons.
- Provide menu commands for import/export, Study, Show Answer, ratings, undo, redo and Back to Decks.
- Disable commands when their action is unavailable, busy, paused or outside its context.
- Route each command through the same coordinator action and guard as its visible control.
- Native text fields, secure fields, card text entry, open menus, dialogs and sheets own their keys.
- Settings must never send review shortcuts to a review behind it.
- Preserve `ReviewInputOwnership`, native focus checks and repeat suppression.
- A held reveal key must not reveal and then rate the same card.
- Do not introduce a second keyboard path that bypasses the existing ownership rules.

## N5. Settings structure

- Use a real macOS `Settings` scene, opened by the standard Settings command and Command-comma.
- Share the app-owned `ReviewCollection` and its coordinators across windows.
- Give Settings a persistent sidebar with exactly Connection and Collection for this preview.
- Restore the last selected section as a device-local preference.
- Connection groups the server address and its test separately from account credentials and account actions.
- Show one visible Server URL label. Show Save Endpoint only after an address change.
- Collection contains the existing file import and export actions and their real presentations.
- Keep Collection's first view to simple action rows. A chosen server action opens its full preparation/decision flow before any request starts; keep all effect warnings before their corresponding preparation or confirmation action.
- Remove the Transfer scope choice. New server imports and exports always include collection and media, without a UI downgrade.
- Keep normal actions, progress and actionable errors visible. Secondary diagnostics belong in N7.
- Keep other Apple platforms' existing Settings navigation functional.
- Preserve storage boundaries and write-on-change behavior (R112, R114).
- Preserve existing backup behavior if touched, including R150's no-media backup note. Do not add unimplemented backup controls.

## N6. Address test and credentials

- Place a compact “Test” button beside the server URL field, outside the text field.
- Its accessible name is “Test Connection”. Test the entered draft address, not a previously saved endpoint.
- Normalize and validate locally before a network request. Reuse existing endpoint eligibility rules, including AnkiWeb denial.
- Retain supported HTTP, HTTPS and path-prefix behavior.
- Report exactly what the check establishes: address validation and connection reachability are separate results.
- A reachability result must not claim valid credentials, server compatibility or a successful collection transfer.
- Report an HTTP response separately from login verification.
- Send no credentials or cookies. Do not follow cross-origin redirects or inspect raw response bodies for diagnostics.
- Testing does not save the endpoint, log in, prepare a replacement or change a collection.
- Disable conflicting address/account actions during a test, transfer or authoritative preview.
- Remember the configured endpoint and saved account identity through the existing persistence paths.
- Keep the device-only login key in Keychain. Never persist the password.
- Do not impose a 60-minute credential expiry on custom servers.
- Preserve logout, endpoint-change invalidation, account matching and recovery-preview endpoint restrictions.

Supporting requirements: R112, R1496–R1502. The current service exposes normalization, login and transfer APIs, but no address-test API.
Add a distinct bounded test seam; never substitute `prepare()` or `login()` for a connection test.
A native non-mutating reachability request can serve this preview without Rust or FFI expansion.

## N7. Connection details and real logs

- Use one disclosure titled “Connection details”.
- Inside it, show useful connection metadata and a plainly titled “Sync Logs” section.
- Do not add a second disclosure for logs.
- Read coordinator events and offline transfer history. Production uses real operation records only.
- Show recorded attempt time, direction, stage, progress when available, outcome and useful sanitized failure reasons.
- Distinguish an observed live stage from the last persisted history phase.
- `preparedMilliseconds` is a preparation time. Do not label it completion time or a complete event timeline.
- Report missing history fields as “Not recorded”.
- Retain enough recent sanitized events to explain current test/login/transfer failure while Settings remains open.
- Allowlist structured diagnostic values. Use typed failure classifications and curated messages.
- Exclude passwords, host keys, tokens, authorization headers, raw errors, request/response bodies and card content.
- Sanitize any displayed endpoint; never expose embedded credentials, queries or fragments through diagnostics.
- Preserve local-review information after transfer failure. State partial or unknown effects explicitly.
- Keep existing history remedies reachable: inspect recovery, cancel unfinished preparation, save retained copies and review/resume media.

Existing data: `stage`, `progress`, `mediaProgress`, `message`, `records`, record `phase`, `boundary`, `acceptance` and `media` outcome.
These support truthful details; they do not supply a persisted timestamp for every stage.

## N8. Transfer and file-operation invariants

- Keep `canPrepare`, `canConfirm`, `canRunMedia`, collection activation guards and generation checks authoritative.
- Preserve the replacement preview, retained copies and separate destructive confirmation.
- Keep the destination and pending local work visible before confirmation.
- Warn that later server edits can be discarded. An export preview does not reserve the server.
- Preserve uncertain-upload blocking, local recovery rules, cooldowns and cancellation semantics.
- Every new server preparation uses collection-and-media scope in both directions. No preparation path may silently choose collection-only scope.
- Keep historical collection-only records and prepared decisions interpretable under their recorded scope.
- Never upgrade, resend or restart a historical operation merely because the default changed. A new all-media operation requires an explicit action.
- Preserve the separate media decision and its bidirectional effects, even when collection direction is one-way.
- Keep partial, cancelled, interrupted, excluded-media and unknown-remote outcomes distinct from success.
- Opening Settings or loading offline history must not start or resume a server request.
- Keep collection file export independent of account login. Preserve its destination lease and stale-generation safeguards.
- Do not rename collection replacement as safe incremental sync or imply both sides are up to date.

Supporting requirements: R1496–R1504, R2046, R2048–R2050, R2054–R2063, limited to the behavior currently implemented.

## N9. Pending backend work

Collection file export must include media. A prepared backend patch exists, but media inclusion remains unverified.
Use plain Export Collection labels. Keep the current pre-export explanation truthful until backend support passes its checks.
Current file export without media remains an open requirement, not a completed all-media export.
New server imports and exports include media through the existing separate media-effect decision.
That decision retains consent, cancellation and recovery. Declined or unfinished media leaves the operation partial.
This bounded change does not resume the larger application goal.

## N10. Native preview evidence

- Use `tools/mg` for the focused native build and smoke checks. Use the macOS VM for UI automation.
- Inspect the exact build with a real collection path or explicitly identified local fixture; never present fixtures as live results.
- Inspect empty, normal, busy, error and long-content states at normal and smaller window sizes.
- Check light/dark appearance, larger supported text, focus, scroll boundaries and plain labels.
- Preserve named accessible actions, contrast and platform hit-target limits (R1977, R1979–R1982).
- Apply R1987's maximum three self-directed render/fix rounds before critic and gates.
- Native preview evidence does not replace the later full suites, independent review, provenance checks or human acceptance.

## N11. Full-app decisions (2026-10-27)

These decisions carry N1–N10 from the macOS preview to every platform and to the units that build the screens. Where plan text says otherwise, this section wins; each subsection names the plan text it replaces. The screen contracts are the purposes in `screen-inventory.yaml`.

### N11.1 Review corners (U175, U177, U179, U182)

Decided 2026-10-27:

- Every platform uses N3's quiet corners. There is no top bar with its own background, no title and no remaining counts.
- Upper left, the exit. Sidebar layout (Mac, iPad and unfolded iPhone at regular width): a labelled "Back to Decks" button with the `chevron.backward` symbol. Compact layout: a close button with the `xmark` symbol, accessibility label and Voice Control name "Back to Decks". Esc does the same when nothing above the card owns it (R243).
- Upper right, leading to trailing: on iPhone and iPad the undo button (U179) and the ⋯ menu button (U182); then the sync status icon, only while sync needs the person (N16.2; N3; `sync.status`'s button, whose states that contract gives, N11.8). The Mac has no on-screen undo or ⋯: undo and redo are in the Edit menu, card actions in the Card menu and the card's context menu (R288).
- The progress bar (R341–R345, when its device setting is on) is a thin native linear progress indicator across the top of the review area, below the corner controls. Replaced by N16.1: the bar is on the bottom edge and always shown.
- The review area's background is the card's computed `body` background (R335), so the user's card styling reaches the window edges. Corner controls and the answer region use system control styles, with a foreground chosen for contrast against that background. Need: preserving user-authored card styling. No other chrome tint.
- The sidebar stays hidden for the whole session; Show Sidebar is disabled during review.
- An in-window panel over the review (Mac: `review.editCurrent`, and `editor.add` opened during a session, both covering the card area) owns its keys like a sheet (N4): while it shows, the review keys, their menu commands' key equivalents and the tap zones are inactive (U181's suspension), and Esc and ⌘W go to the panel's close guard, never to Back to Decks.
- Replaces: R333's ✕ and trailing undo/⋯ on Mac, R336's "✕ is the only exit", R337's and R21's re-showing of the sidebar during a session, U177's layout bullet.

### N11.2 Show Answer and answer buttons (U178, U180, U181)

Decided 2026-10-27:

- A bottom action region of one answer row's height is reserved on both sides (N3), above the home indicator and outside the fold region (R9).
- Question side: one "Show Answer" button in that region on every platform, in the prominent style. Taps and clicks on card content and the tap zones still reveal (R346, U180).
- Answer side: R351–R356's four buttons. No key hints on any button, on any platform (N4). Shortcuts show beside the menu commands (Mac, iPad menu bar), in the iPad ⌘ overlay and in Help › Keyboard Shortcuts. Good has the prominent style. Space and Return answer Good only through the review key map and its setting (R134), never through a default-button key binding, so there is one key path (N4).
- Replaces: R346's "The question side shows no buttons", R184's limit of the Show Answer button to assistive technologies, R357, U178's key-hint bullet.

### N11.3 Menu commands (U159, U181)

Decided 2026-10-27:

- Now, before U159: N4's commands are in the menu bar: Import… and Export… (File), Study (selected deck), Show Answer, Again, Hard, Good, Easy and Back to Decks, each disabled outside its context and routed through the action and guard of its visible control. One Study menu holds these study commands until U159 lands.
- Undo and Redo, from now on: they replace the system Edit menu's undo group with R173's titles and route through the focused undo target. They never appear in the Study menu.
- When U159 mounts R280–R285: Study moves to the Deck menu (R283); Show Answer, the ratings and Back to Decks to the Card menu (R284), "Back to Decks" replacing "Close Review". Other R284 and R339 items arrive with their units.
- A menu item shows its unmodified key, but the card page keeps the first claim on that key (R426) and N4's input ownership holds. Spike S3 (U181) picks the mechanism; a key equivalent that takes the key before the page is not allowed.
- Replaces: U181's deferral of these commands to U159, and the Study menu's Undo Answer and Redo Answer items of the current app.

### N11.4 Settings (U160, U161, U163, U163a, U164)

Decided 2026-10-27:

- Mac: N5's separate Settings scene is the whole app's Settings, opened by Meguri › Settings… (⌘,). Its sidebar sections, in order: Account & Sync, Review, Editing, Browsing, Appearance, Heatmap, Collection, Anki Server, Backups, Maintenance, Integrations, About. A section whose unit has not landed is absent (U152's registry). A search field above the sections filters by name and keywords (R111). The Mac main window has no Settings sidebar section.
- Accessibility attachment: the native `.searchable` sidebar field and its Search button lacked identifiers in VM drive `20261030T001853Z-95240`. `SettingsSearchIdentifiers` attaches only `settings.root.search` and `settings.root.searchButton` through public AppKit/NSAccessibility APIs within the owned sidebar column, on native mount/layout. The search button is matched to its public cell's native localized accessibility label. This preserves the native search control, labels, roles, layout and keyboard behavior; exact native hierarchy and remount verification remain required for the repair.
- Collection keeps N5's file import and export and the Anki Transfers actions (R118). The preview's Connection section becomes Anki Server, with N6 and N7 unchanged; the new name keeps it apart from Account & Sync.
- Each section is a grouped form. Changes apply at once, with no Save button (R115). The selected section is restored as a device-local preference (N5).
- Synced marker (R113's "Synced" globe badge), the one form for every section: the `globe` SF Symbol in secondary style after the row's label, no capsule, background or tint; VoiceOver reads "Synced" with the row. R113's footnote ends the footer of a group with synced rows. `stats.heatmapSettings` and `settings.backups` use it.
- iPad and iPhone: Settings stays a sidebar section (regular width) or a tab (compact) of the main window (R109), listing the same sections as rows in the same order; Integrations is Mac only.
- About is a section on every platform. On Mac, Meguri › About Meguri shows the standard About panel.
- Backups: a Settings section on every platform; on Mac also File › Back Up Now and File › Restore from Backup… (R280). The restore sheet opens on the window its command came from; from the damaged-collection screen, on the main window (`settings.backups`).
- Replaces: R6's single window for Settings, R109's "Mac included; ⌘, selects it", R110, R1661's Settings placement on Mac (the Tools menu stays), U160's "Sidebar layout: a section with page list" for Mac, U152's test "the sidebar shows Home, Settings and the Decks group" for Mac, R151's and U163a's "Show Backups" label (now "Show in Finder" or "Show in Files"), and U163a's "this unit adds no menu" (it adds the two File items). N5's "exactly Connection and Collection" remains the preview's scope only.

### N11.5 Deck workspace and import (U152, U155, U162, U228a, U228)

Decided 2026-10-27:

- Decks root (the detail area with no deck selected; the compact Decks tab): one native List. A deck row is its last name component in ordinary weight, with system disclosure for children (R36), and trailing secondary text with the non-zero counts in New, Learning, To Review order, for example "20 new · 3 learning · 12 to review"; all zero shows no count text. No column headers and no count colours in deck lists.
- Mac and iPad sidebar deck rows: the name and a native badge with the To Review count, hidden at zero. The accessibility value gives all three counts with their names (R182); hover help gives the full path (N2).
- Deck page: the three counts labelled New, Learning and To Review, with the numbers in the system blue, red and green (R32 kept here only: labels always shown, contrast checked) and the buried "+N" (R71).
- Count and state colour contrast (accessibility exception; need: R32's 4.5:1, which the system blue, red and green do not all reach, for example the system green on a light background is about 2.2:1): each number, and each state-coloured text of `cardInfo.card`, uses the system colour where it reaches 4.5:1 against its background, otherwise that colour's increased-contrast variant, otherwise a darker (light appearance) or lighter (dark appearance) shade of the same hue, the least change that reaches it, kept as an asset colour with its measured ratio. Checked in light, dark and Increase Contrast by the snapshot contrast check and the system accessibility audit, at the text's rendered size.
- Activity (studied-today line, heatmap, streak stats; R28, R43, R44): a section of the same List after the deck rows, on every layout; never a side-by-side panel. On the deck page it follows the counts and Study.
- Import is in File › Import… (Mac, iPad menu bar), Settings › Collection, the empty-collection state and onboarding, file drops and Open in Meguri (R51, R52). Home, the deck page and the empty-deck state have no Import File action; the empty deck offers Add Note once U195 attaches it.
- Replaces: R29, R30's columns and headers, R32 for deck lists and the sidebar, R33, R182's count names "New/Learn/Due" (now New, Learning and To Review), R1299's Mac and iPad placement beside the deck tree, R50's Import File, R80's Import File, U152's layout bullet and its bottom Import File action.

### N11.6 Session end (U157, U186)

Decided 2026-10-27: Back to Decks after at least one answer restores the deck workspace and shows the session summary over it as a sheet (`home.sessionSummary`); with no answer it shows none (R91). The summary has no chart.

### N11.7 Stats header and graph boxes (U225, U226)

Decided 2026-10-27:

- R1350's sticky header is the window's toolbar, which stays in place while the graphs scroll. No bar of its own, no custom background.
- Regular width (Mac, iPad when the controls fit): in order, the scope picker (segmented: Deck, Collection, Search), the deck pop-up button (Deck scope) or the search field (Search scope), and the history picker (segmented). Compact width, or when they do not fit: the navigation title "Statistics" with the scope and history as its subtitle, and one toolbar menu holding the scope, the deck list and the history; the Search scope shows the navigation bar's search field.
- The history labels are "Last 12 Months" and "All History", in title case as Apple's control labels are; `stats/graphs-common` G3 keeps Anki's lower-case wording as its source.
- Each graph is a system group box: Anki's title and subtitle (R1358), the graph's controls, the chart and its table. No custom card background, corner radius, shadow or motion tokens; system animations, which Reduce Motion already shortens (R1368).
- Replaces: U225's "sticky top bar" and `ChartCard` chrome with DesignSystem motion tokens, and U225's segmented scope control at compact width.

### N11.8 iCloud screens (U151, U161, U166)

Decided 2026-10-27 (screen contracts `sync.status`, `settings.accountSync`, `sync.connect`, `sync.publicationChoice`, `sync.replaceLocal`, `sync.replaceCloud`, `sync.reviewConflicts`, `onboarding.welcome`):

- One iCloud status button: trailing in the Mac main window's toolbar and the iPad sidebar layout's toolbar, trailing in the compact Decks navigation bar (R102), and in the review's upper-right corner (N11.1). It opens a popover; at compact width a sheet with medium and large detents. A distinct SF Symbol per `sync/policy` P14.2 state, never colour alone (R104). While media transfers, a quiet footnote-style indicator sits beside it (R106).
- Sync Now has one path: the popover's button, File › Sync Now (⇧⌘Y) and the global Y key, all through `syncNowAvailability()`. The unmodified Y acts only when no text field, secure field, open menu or other view that owns keys has focus (N4, N11.3). From the menu or Y in the sidebar layout the popover opens to show progress (R105).
- Account & Sync's enable control, with no confirmation (`sync/cloudkit-recovery` V16), is the first that applies: Resume iCloud Sync while the user's pause is on, whatever the binding; none while a publication decision is pending, whose Choose… opens the publication choice; Resume Syncing for a `stopped` binding that V9.2 offers to resume; Connect to iCloud… while no binding is usable; otherwise Pause iCloud Sync. Stop Using iCloud on This Device… is a separate action with an alert, in its own row shown while a binding is active whatever the account status; while an `adopt` or `publish` op exists it opens no alert and shows that the op must finish first, since the op's Cancel or failure would restore the binding before. Advanced holds only the replacement and restore entries that `sync/status-strings` T14 lists.
- While the user's pause is on, no iCloud preview read runs (V26.1): every entry that needs one (Connect to iCloud…, Resume Syncing, Advanced's entries, the attention actions that open a replacement preview, the publication choice's Replace) stays visible and disabled with the paused line as its reason.
- Review Conflicts is a page pushed inside Settings › Account & Sync on every platform; the status popover opens Settings at that page. No separate window. During a review session on iPhone and iPad, the popover's Review Conflicts…, Show Details… and Account & Sync link present Account & Sync in a sheet over the review, already at the chosen page, instead of switching the main window's tab or sidebar section (N11.1 keeps the sidebar hidden); the sheet owns the keys (N4) and closing it returns to the same card. The Mac opens its Settings window as at any other time.
- Replacing a whole collection: the entry (a label ending in "…") opens a preview sheet and changes nothing; the preview's destructive button is the confirmation. This is how N8's "separate destructive confirmation" applies to iCloud replacements: the entry only opens the preview, so the preview's own destructive button is the separate confirmation, and no second alert follows. The two-collections decision and the publication choice push the chosen direction's preview inside their own sheet. Recovery choices and destructive buttons are never the default button: Return confirms no replacement, and Esc cancels while cancelling is allowed. In a preview's toolbar, Cancel uses the cancellation action placement and the Replace button the destructive action placement with the destructive role; never the confirmation action placement (on a Mac sheet it makes the default button, bound to Return) or a default action shortcut. This includes the first connection's "Use iCloud Collection" for an empty collection (V13.3 row 4), since an empty collection may still hold note types, presets and settings (V1).
- N8's "Opening Settings … must not start or resume a server request" covers Anki server requests. Showing Account & Sync's Advanced group may start the read-only iCloud preview reads of `sync/cloudkit-recovery` V26.1, which decide which replacement entries are offered; they write nothing and send no change.
- The publication choice (V12.2) opens by itself once, over the deck workspace, when a restore, a `.colpkg` import or a manual Anki import completes while iCloud is enabled on this device. Not Now closes it; the status keeps the action.
- All sync text comes from `sync/status-strings`; the screen contracts name its keys.
- The sync status icon of N11.1's upper-right corner is `sync.status`'s button; its states are that contract's.
- Replaces: R280's menu title "Sync" (now "Sync Now", `sync.now.command`), and U161's "Review Conflicts is reachable from …" for placement only (its two entry points stay).

### N11.9 Deck options (U232, U233, U234)

Decided 2026-10-27 (screen contract `deckOptions.options`):

- Placement follows R730, except during a review session: on every platform the screen is then a sheet over the review (N11.1 keeps the sidebar hidden), which owns the keys (N4); closing it returns to the same card.
- One grouped form. The preset picker and a Preset Actions menu lead it (R731, R732); Save to All Subdecks is in that menu and saves at once. Save is the toolbar's confirmation action (⌘S, ⌘↩).
- Row parts are system controls: "Global" as secondary text after the label (R735), a borderless Restore Default button and a help button with a popover (R734). No badge capsule, custom banner or token colours. Issues show under their row with one symbol per severity, never colour alone, and the issue text in the primary label colour: an error `xmark.octagon` and a strong warning `exclamationmark.octagon.fill`, both in the system red; a warning `exclamationmark.triangle.fill` in its multicolour rendering, whose dark mark keeps the symbol readable where the system yellow alone is below 3:1 against a light background.
- Changes from elsewhere while the draft differs: a row at the top of the form with Reload and Keep Editing.
- On iPhone, while the draft differs, Cancel replaces the back button and swipe back is off, so leaving always asks (R734).
- A value outside its editor range shows `sched/deck-options` O4's error and disables Save; it is not silently refused.
- Wide windows show the sections in two columns (R738); the R1988 variants vary only that arrangement.
- Accessibility identifiers take the inventory id as their prefix (`deckOptions.options.*`, with `deckOptions.options.cancel` for the sheet's Cancel).
- Replaces: R730's placement for the review entry; U232's KTD20 tokens and "non-modal banner"; U232's identifiers `deckOptions.*` (now `deckOptions.options.*`); U232's "Default (id 1) shows no Remove" and its test "Remove absent from the menu" (Remove Preset… is shown disabled for Default, O6.5); "save-to-all-subdecks mode" among U232's draft-only operations (Save to All Subdecks saves at once, O7); U232's "Controls only accept in-range values" (an out-of-range value shows O4's `out_of_range` and disables Save); the "Global badge" of R735, U232, U233 and U234 (plain secondary text "Global" after the label); the "rejected" outcomes in U233's and U234's test scenarios (a refused value is now an O4 error that blocks Save).

### N11.10 Note types (U200, U201, U203)

Decided 2026-10-27 (screen contracts `noteTypes.list`, `noteTypes.templates`, `noteTypes.fields`, `noteTypes.changeNoteType`):

- Settings › Collection holds U160's Note Types row (R915, R18). On Mac it brings the main window forward on the Note Types section. N11.4's section list is unchanged.
- The template editor and the field editor are sheets from every entry, the Note Types list included: page-sized on Mac and iPad, full screen on iPhone. On iPhone a note type row pushes that type's page of actions.
- No schema-change confirmation (U200's structural save, corrected 2026-10-27; `notetypes/schema-changes` G4). The questions that remain belong to the change: Add Card Type's card count, Remove Card Type's removal preview, R918's delete question, R914's restore question (Cancel is the default), R922's field delete question, and Change Note Type's loss section, which Save accepts.
- A successful Save closes the template editor and the field editor; on errors they stay open with the draft. While a save op or a restore from the editor runs, the sheet stays, disabled, with its progress (and Cancel while it plans), until the op ends; the work runs off the main thread (R927).
- Accessibility identifiers take the inventory ids as their prefix (`noteTypes.list.*`, `noteTypes.templates.*`, `noteTypes.fields.*`, `noteTypes.changeNoteType.*`).
- Replaces: U200's identifiers `notetypes.row.<id>` and `templates.editor.front`, and U200's in-window sheet rule, which now also covers Fields… and Cards… opened from the Note Types list.

### N11.11 Mac quit (R127, R1557)

Decided 2026-10-27 (screen contract `sync.quitFlush`):

- Before the flush, every open screen whose close asks shows its own prompt in turn (`editor.add`, `deckOptions.options`, `noteTypes.templates`, `noteTypes.fields`); Keep Editing cancels the quit (terminateCancel, also for a logout or restart), Discard closes that screen. A running note type, Change Note Type or deck options save is waited for. The prompts have no time limit; the 10-second limit starts with the flush. Then other sheets on the main window, which hold no draft, are closed, the review session ends and the editors flush (`sync.quitFlush`).
- `sync/policy` P11.4's quit flush shows as a window sheet on the main window, brought forward or opened again. Other windows and commands are disabled meanwhile.
- Its one button is "Quit Without Syncing" (`sync/status-strings` T15), the cancellation action (Esc), never the default. A second Quit command acts as that button.
- QuitNow shows nothing. iPhone and iPad have no quit sheet.
- Replaces: R1557's "Quit now" label.

### N11.12 Heatmap settings (U229)

Decided 2026-10-27 (screen contract `stats.heatmapSettings`):

- Settings › Heatmap (N11.4) holds R1339's group, First day of week (`editor/config-keys` K5) and R1342's device toggles in their own On This Device group.
- Calendar mode is a segmented picker. The day limits are written when their field commits, not per keystroke. Turning Ignore data before on starts at the collection's creation date, which `stats/heatmap-addon` A2.2 ignores, so nothing changes until a later date is chosen.
- Replaces: U229's identifier pattern `settings.heatmap.*` (now `stats.heatmapSettings.*`).

## N12. Mac redesign decisions (2026-11-01)

Decided by Antoine in chat on 2026-11-01. Where earlier text differs, this section wins.

### N12.1 Deck page

- The Mac deck page is the album page: leading large title, one line of counts ("20 new · 3 learning · 72 to review"), a Study button, then Subdecks, the deck's error and its previous session in one native List.
- All three counts stay visible, including zeros, in plain secondary text. VoiceOver reads each as New, Learning or To Review with its number.
- The Decks root has no list header and the sidebar has no "Decks" header. Its first row is "All Decks".
- Replaces on Mac: N2's centred deck and centred Study, N11.5's coloured counts on the deck page.

### N12.2 Buttons

- No large capsule buttons in app content. Use standard Mac control sizes and shapes; window toolbar items keep the system's own shape.

### N12.3 Review

- The default light card page is white (`render/card-runtime` C4.3). A template's own background still wins, and `--canvas` keeps its value for templates that read it.
- Back to Decks and the status icon are real window toolbar items, with no window title. The window behind them uses the default card page colour.
- Back to Decks is a round icon-only toolbar button (`chevron.backward`). "Back to Decks" stays its accessible name and hover help. Replaces N3's and N11.1's visible label on Mac.
- Answer buttons are standard-height Mac buttons with the name and interval on one line, four equal widths, Good prominent.

### N12.4 Anki server sync

- Legacy Anki server sync stays hidden on every platform. Account & Sync offers iCloud Sync and Meguri Sync using Meguri's own server under `sync/meguri-provider`. Settings shows no Anki Server section and Collection shows no Server Transfers.
- Antoine's 2026-11-16 own-provider exception uses MP1–MP5 for service choice and provider-specific setup/status, with MP2's native controls. N11.8's iCloud controls and safety rules remain. Legacy transfers stay frozen; no AnkiWeb contact is authorized.
- `.colpkg` file import and export stay.
- The code, its tests and the `settings/anki-transfer/*` routes stay. UI suites that open the hidden screens launch with `-ShowAnkiServerInterface YES`.
- Pauses N5's Connection section, N6, N7 and N11.4's Anki Server section until Antoine brings the feature back. R118's Anki Transfers actions and the requirements behind them are not met while it is hidden.

## N13. iPhone and iPhone Duo design (2026-11-01)

Decided by Antoine in chat on 2026-11-01, from captures on the iPhone Duo simulator (iOS 27.1). Where earlier text differs, this section wins. Sizes in points: outer display 466 × 678; inner display 951 × 669 held wide and 669 × 951 held tall, with a 40-point fold through the middle.

### N13.1 Layout

- The iPhone Duo uses the tab layout on both displays: the system places the tabs and toolbars for each position, with no overrides. Replaces R4's "unfolded foldable iPhone" and AE7's and R576's switch to the sidebar layout. iPad keeps the sidebar layout (R4).
- The shell chooses by interface idiom: an iPhone keeps the tab layout at any width (R3). A size-class-and-fold test would pass through the sidebar layout for a frame while unfolding, dismissing the review's cover.
- The answer row reads the fold from `reservedRegions(kind: .division, options: .includeInactive)`. `.includeInactive` matters: without it a fully open display reports no fold.
- Inner display held wide: Decks shows the deck list beside the deck page. Held tall: one stack, list then page. The system hides a split view's list behind a toggle when held tall, which leaves "Select a Deck" as the first screen.
- Unfolded, Go to Deck and Create Deck stay with the list and don't repeat on the deck page.

### N13.2 Decks

- Touch toolbar items have a symbol and a title. Text-only items stay in a horizontal bar on the Duo.
- Touch deck rows: the name and one trailing secondary number, new + learning + to review, hidden at zero. The accessibility value keeps all three counts. Replaces N11.5's row counts on touch.
- Deck actions are a long press on touch; the row has no … button. Apple reserves the ellipsis for overflow.
- On a stack, a subdeck on the deck page pushes, so Back returns to its parent.
- The Decks root has no list header on touch either (N12.1).
- The touch deck page is N12.1's album page, in a plain list on white, with a standard-size Study button.

### N13.3 Review

- Close, Undo, Redo and the status icon are system toolbar items, so they follow each position. A touch review has a white page. Replaces N11.1's custom corners on touch; the Close symbol and name are unchanged.
- The counts and thin progress bar sit at the top of the content, with the timer beside them. Replaced by N16.1 and N16.7.
- Answers are one row at the bottom in every fold position: standard rounded-rectangle buttons, name over a small interval, Again, Hard, Good, Easy. Replaces N11.2's prominent Good on touch.
- Held wide unfolded, the row splits at the fold: Again and Hard on one side, Good and Easy on the other, in every hinge position, so nothing moves when the phone opens further. Held tall the fold crosses the card, not the row. Nothing interactive sits on the fold (R9).
- Show Answer takes the trailing half of the row, over Good and Easy. It's smaller than a full-width button and keeps the thumb in place. Replaces N11.2's prominent Show Answer on touch. Replaced by N16.3 where no fold crosses the row. At accessibility text sizes the answers stack; with a fold in the row, they stack in two columns either side of it.
- One view structure holds the card in every fold position, so folding never recreates its page (R576).
- Not chosen, after captures: a book layout (card on one side of the fold, answers on the other), an answer column at the side, and, held tall, a 2 × 2 grid or answers filling the lower half.

- The touch-target check leaves navigation bar buttons to the system's hit-region audit. iOS draws them 36 points tall and forcing 44 would override the system bar. Replaces the "app's own bar buttons are measured" rule.

### N13.4 Not covered yet

- U168's UI tests: nothing in `simctl` or XCUITest can change the fold position, so positions are checked by hand in Device Hub (`tools/vm/duo-fold.sh`). `FoldAvoidingRow` has unit tests. Its code is in DesignSystem, not U168's `app/Meguri/Shell/FoldAwareLayout.swift`, because review and the shell both need it.
- Foldable snapshot cells stay deferred until the snapshot job renders on the foldable lane's iOS 27.1 runtime.
- A card's own background doesn't yet reach the toolbar and answer row (N11.1, R335).

## N14. Round 1 structure decisions (2026-11-19)

Decided by Antoine in chat and page comments on 2026-11-19 (`decisions.md`). Where earlier text differs, this section wins.

### N14.1 Today

- Sidebar layouts (Mac, iPad at regular width): the sidebar's first row is "Today" with the `sun.max` symbol, in every collection state. With no deck selected the detail shows Today, not a second deck list. The sidebar remains the only deck tree.
- iPhone Duo held wide unfolded: the detail beside the deck list shows Today until a deck is picked. The list beside it then leaves out its Previous Session section, and Today leaves out the collection's error line, which the list shows: nothing appears in both columns.
- Today: the collection's totals ("20 new · 3 learning · 72 to review", the top-level decks' counts), then, with no section header or rule, the decks with cards waiting, each with a Study button that starts that deck through the deck page's Study path. A deck is listed when it has cards waiting and no subdeck with cards waiting; a parent is also listed, before its subdecks, when it has cards of its own beyond theirs. A nested deck shows its parents as secondary text under its name ("Languages › 日本語"); the list has no indentation. Mac rows show the non-zero counts by name; touch rows show one number (N13.2), which VoiceOver reads as "N cards waiting". At the accessibility text sizes a row stacks the count and Study under the name. Study keeps the deck page's minimum target.
- With nothing waiting, Today shows only "Nothing to study right now": no totals and, on Mac, no subtitle (N14.4). Only the previous session keeps a section label. The error line and the previous session follow.
- Compact layouts (iPhone, the Duo folded or held tall) keep the deck list as the Decks root and have no Today.
- Loading, no collection, failed and no-decks states keep their content; the page is still titled "Today" where the sidebar's first row says so.
- A selected deck that is no longer in the collection (deleted, or removed by a sync) loses the selection: the sidebar layouts return to Today. A hidden deck keeps its page.
- iPad's Today stays a plain list while its deck page is grouped (N14.3); Antoine accepted both on the iPad capture.
- Replaces: N11.5's Decks root as the sidebar layouts' detail, N12.1's "All Decks" first row, N13.1's "Select a Deck".

### N14.2 Tab-layout deck list

- The toolbar has the sync status button, a Search button (`magnifyingglass`, `home.collection.search`) and one Add menu (`plus`): New Note and New Deck…. There is no Go to Deck or Create Deck button and no Study bar.
- Search presents the system search field ("Search or Create a Deck") with the keyboard; until then the field stays out of the way. The title is N14.8's. Typing replaces the tree with the matching decks, using Go to Deck's filter: each shows its last name component with its parents as secondary text underneath, as Today does; a row opens that deck. When no deck has the typed name, a "Create deck named …" row opens the Create Deck sheet with the name filled in.
- The query and the field's presentation are navigation state: a fold, an unfold or a layout switch keeps them.
- The Go to Deck sheet stays for the sidebar layouts' toolbar, the "/" key and the Mac's Deck menu.
- Replaces: N13.2's "Touch toolbar items have a symbol and a title" for this list, and N13.1's "Go to Deck and Create Deck stay with the list".

### N14.3 Deck page

- In the tab layout the deck page's toolbar has Add Note only. Sidebar layouts keep Go to Deck and Create Deck in the window toolbar.
- A regular deck with no new, learning or review cards waiting shows "Nothing to study right now" in place of the counts and an Add Note button in place of Study. A filtered deck keeps its counts and Study. The deck menu's Study and the Mac's Study command are disabled under the same condition, through the same availability function.
- No New Subdeck button and no More menu. The long-press deck menu is otherwise unchanged.
- iPhone, the Duo and iPad: the page is a system inset grouped list on the grouped background. Its first section has the deck's last name component as a large in-content header over one row: leading, "N cards waiting" (new + learning + to review) with "X new · Y learning · Z to review" in footnote secondary text under it; trailing, the standard-size Study button. With nothing waiting the row reads "Nothing to study right now" with Add Note trailing. At the accessibility text sizes the action goes under the text (by text size, not by fit: under a fit test the Dynamic Type audit failed the row's elements), and the three counts go one per line before any of them wraps. Then the "Subdecks" section, the error line, and the previous session in its own section. The navigation bar's title mode is inline, so no empty large-title band sits above the page. Replaces N13.2's "plain list on white" for the touch deck page; the Mac page is N14.4's.

### N14.4 Mac titles

- Today and the deck page name themselves in the window toolbar. The title is "Today" or the deck's last name component. The subtitle is the counts, zeros included, or "Nothing to study right now" on a deck page; on Today it is the totals, and nothing at all while nothing waits (N14.1). The deck page's content starts with Study at the list's ordinary inset.
- Touch layouts keep the large title in the content (deck pages) or use N14.8's (tab roots).
- Replaces on Mac: N12.1's leading large title and in-content counts line.

### N14.5 Activity (not built)

- The heatmap is the minimal form of `decisions.md` (2026-11-19). On a deck page it follows the subdecks, not the counts and Study. Replaces N11.5's "On the deck page it follows the counts and Study" when the heatmap is built.

### N14.6 Sidebar deck list

- Under the Today row, the deck rows sit in a section with the system label "Decks", on Mac as on iPad. Replaces N12.1's "the sidebar has no Decks header".
- Deck rows have no symbol; an emoji in a deck's name stays as the user typed it.
- A deck with no new, learning or review cards waiting shows its name in the secondary label colour (on iPad the `textSecondary` token, which keeps 4.5:1 on the sidebar). No check mark or other mark. While the row is selected its name takes the primary label colour, so it reads on the selection highlight; the highlight itself is the system's. The row's accessibility value gives the three counts by name on Mac and iPad, which also says when nothing waits.
- The count is the To Review badge, hidden at zero (N11.5).
- The disclosure control is the one the system's sidebar list provides, unmodified: leading on Mac, trailing on iPad.

### N14.7 Touch deck rows

- A deck row's disclosure triangle is on the leading edge, before the name: the system's own disclosure indicator, at its size and weight, at the row's normal leading inset. The layout reserves only the indicator's width; rows without subdecks keep that width empty, so names line up, and each nested level indents by the standard spacing. The count ends at the row's trailing edge with no reserved column.
- The touch target is a 44 × 44 point button laid over the indicator, above the row's content: it reaches into the row's leading inset and a little over the name's start, where it wins the tap. It reserves no layout. The drawn indicator has an empty accessibility representation, so that button is the only disclosure element.
- Selection and disclosure stay separate controls with their identifiers (`home.deck.row.<id>`, `home.deck.disclosure.<id>`). Applies to the deck list and to the deck page's Subdecks rows.
- Replaces N13.2's trailing disclosure and its reserved count column.

### N14.8 Tab roots' titles

- In the tab layout, the Decks list and the Settings root use the system's inline large title (`ToolbarTitleDisplayMode.inlineLarge`): the title sits at the leading edge of the navigation bar, on the toolbar items' row. No custom title view. While search is presented the system's field takes that row. Deck pages keep the inline bar with the deck's name in the content (N14.3).

### N14.9 iPad sidebar title

- The iPad sidebar's navigation bar draws no title: the app's name is already in the status area. The column keeps "Meguri" as its navigation title for accessibility, removed from the bar with the toolbar's own API (`toolbar(removing: .title)`). The sidebar toggle stays where the system puts it, except during review, which has none (N16.2). The Mac sidebar never drew one.

### N14.10 Today row beside Today

- iPhone at regular width with the list beside the detail (the Duo unfolded and held wide): the deck list starts with a "Today" row in a group of its own, above the decks. It has the sidebar layouts' `sun.max` symbol, the title "Today" and, trailing, the cards waiting across the collection (new + learning + to review of the top-level decks), hidden at zero like a deck row's number.
- It is selected on arrival, while the detail shows Today. A tap returns the detail to Today after a deck was picked, as the sidebar's Today row does.
- The selected row, Today or the deck whose page shows, has the system's grey selected-cell fill; its number takes the primary label colour there. A deck row is marked this way only in this list. When a collapsed deck hides the selected subdeck, the collapsed deck's row is marked.
- The Today row is the top of the list: scrolling to the top, and returning to a list that was at its first deck, show it. Its target is at least the minimum height.
- Compact width (an ordinary iPhone, the Duo folded or held tall) has no such row and marks no row. iPad and Mac keep the sidebar's own Today row (N14.1).
- Search results replace the whole list, the Today row included (N14.2).
- Accessibility: `home.today.row`, labelled "Today", with "N cards waiting" as its value and the selected trait; the selected deck row has that trait too.

## N15. Round 2 Settings and sync decisions (2026-11-19)

Decided by Antoine in chat and page comments on 2026-11-19 (`decisions.md`: `settings.root`, `settings.accountSync`, `sync.connect`, `sync.status`, `sync.serviceSwitch`). Where earlier text differs, this section wins. The behaviour is `sync/policy` P25, `sync/cloudkit-recovery` V27 and `sync/meguri-provider` MP6; all text is `sync/status-strings` T19. Those spec sections were approved on 2026-11-19 (`docs/process/review-log/redesign-specs-20261119.md`). The points raised for the owner are in P25.6, V27.9 and MP6.8; two of them (no device backup of a replaced iCloud collection, and Cancel removing the Meguri Sync sign-in) were decided on 2026-11-19 and are marked there.

### N15.1 Settings page

- Settings is one flat page: a grouped form whose sections are Sync, then Collection. The sections' rows are on the page itself; no row pushes a form that only repeats a section.
- Mac, and the iPhone Duo's inner display held wide: the sections are a list beside the selected section, as in the system's Settings. Mac keeps N5's Settings scene and N11.4's search field; the list has Sync and Collection. The last selected section is restored (N5).
- iPhone, the Duo folded or held tall, and iPad: the flat page, in the Settings tab or sidebar section (N11.4), with N14.8's title in the tab layout.
- A Settings section that N11.4 lists and whose unit lands later joins as a further section after Collection, in N11.4's order. This section decides only Sync and Collection.
- Collection keeps N5's and N12.4's file import and export. Import and export always carry cards and media; there is no media-scope choice (N5).
- Native controls only: `Form`, `Section`, `Picker`, `Toggle`, `LabeledContent`, `NavigationLink`, sheets and alerts with system roles. No custom chrome (N1).

### N15.2 The Sync section

Its rows, in this order, each only under its condition:

1. **Service**: a standard `Picker` labelled Service with Off, Meguri Sync and iCloud, in that order. iCloud has the secondary label Beta after its name, in the picker and wherever the service is a heading or a row label; the label is text, not colour alone, and VoiceOver reads "iCloud, Beta". The picker shows the service the core reports, never the value just tapped (MP6.1).
2. **Account**, for Meguri Sync only: the username as its value; it pushes the page with Server Address and Username, and the sign-in fields while not signed in (MP2's labelled fields and one prominent Sign In, N6's credential rules). iCloud has no account row: the status says when the Apple account needs the user, and Meguri never asks for an Apple password (R117).
3. **Status**: the status symbol, title and detail of T19.4 or T19.5. A needs you status has its one action as a button in this row. An issues row under it pushes the issues list. A waiting status has no button.
4. **Sync Automatically**: a `Toggle` with its footer, shown while a service is selected. Until the service is connected (selected, with its setup completed: a ready binding, `sync/policy` P25.3) it is disabled and shows its stored value (decided 2026-11-19 (review F8)).
5. **Sync Now**: a button, through `syncNowAvailability()` (N11.8); disabled with its reason as N11.8 says.
6. **Troubleshooting**: a `NavigationLink`, shown while a service is selected.

- While the value is Off the section has the Service row and its footer only.
- Changing the value: from Off, no confirmation; setup starts, with progress and Cancel under the picker. From a service, the `sync.serviceSwitch` sheet once, for Off or for the other service; then the new service's setup runs with no Connect step (V27.2, MP6.3). While a change is refused, the picker keeps its value and the reason shows under it.
- No row or footer says that cards and media sync together. The section has no media toggle, no Pause, no Sign Out, no Stop Using row, no Advanced group, no Review Conflicts row and no "This option is no longer available." line: an action that cannot run now is hidden, or disabled with the reason it gives; an action with no implementation is not shown.
- The iCloud privacy footer (R2058, R2059) stays under the section while iCloud is the service.

### N15.3 Which collection to keep

- One sheet (`sync.keepCollection`) for both services: both sides' counts and last change side by side, then two buttons, each with what it replaces and what protects the replaced side under it, and Cancel.
- Both buttons have the destructive role and neither is the default; Return confirms nothing and Esc cancels (N11.8). A button is the confirmation: no preview page and no alert follow it.
- It opens from setup only when both sides hold a collection, from a needs you action, and once by itself after a restore or import that needs the choice (N11.8). Cancel during setup leaves the Service at Off.
- With one direction only, it is the short upload or download confirmation of T19.3.

### N15.4 Problems in three kinds

- **Waiting**: the status detail says what Meguri waits for. No button.
- **Needs you**: the title, one sentence and one button, in the Sync section's status row and in the status popover. With several, one shows at a time, in T19.4's order.
- **Issues**: one row with their count, in the Sync section and the popover, which opens a list. Each row of the list has its text and the items it concerns.
- The sync status button (N11.8) uses the needs-attention symbol only for needs you. With only waiting or issues rows it keeps the symbol of the state underneath; an issues count is never shown by colour alone. During review the button shows only as N16.2 says.
- iCloud's conflict copies have no entry in the status, the popover or the Sync section.
- While the provider is blocked the Service control stays enabled, study, backups, import and export keep working, and the status names Export Collection as the way to save everything (MP6.5; decided 2026-11-19 (review F5)).
- During a review session on iPhone and iPad, a status action that needs Settings presents the Sync section in a sheet over the review, as N11.8 says for Account & Sync.

### N15.5 Troubleshooting

One page inside Sync, for both services, with these rows:

1. Replace This Device's Collection…
2. Replace the iCloud Collection… or Replace the Meguri Sync Collection…
3. for iCloud, Restore Previous iCloud Collection from <date>…, while it is offered
4. Cancel Replacement…, while a replacement can be cancelled and that is not the status action
5. Sync Log, a page of real recorded attempts under N7's rules: nothing fabricated, missing fields as "Not recorded", no secrets or card content
6. Copy Debug Info
7. for iCloud, one plain line with the number of saved conflict copies, with no action, while there are any
8. Show Replaced Media, only while replaced-media copies exist: it opens their folder in the Files app, or in Finder on Mac. Its footer says the copies stay until the person deletes them (V27.7; decided 2026-11-19 (review F7))

- Each replace entry opens its preview sheet and changes nothing; the preview's destructive button is the confirmation (N11.8, N8).
- Opening the page may start iCloud's read-only preview reads (N11.8); it starts no Meguri Sync request (N8, MP2).

### N15.6 What this replaces

- **N11.4**: "iPad and iPhone: Settings … listing the same sections as rows" becomes N15.1's flat page; Account & Sync in the section list becomes Sync. The Mac scene, its search field, "Each section is a grouped form", "Changes apply at once", the synced marker, About and Backups are unchanged.
- **N11.8**, third bullet (Account & Sync's enable control, Resume iCloud Sync, Connect to iCloud…, Pause iCloud Sync, the Stop Using row and Advanced): replaced by N15.2 and N15.5. Its rule that Off is refused while an `adopt` or `publish` op exists stays (V27.5).
- **N11.8**, fourth bullet (entries disabled while the user's pause is on): removed; there is no Pause (P25.2).
- **N11.8**, fifth bullet: the Review Conflicts page and the popover's Review Conflicts… and Show Details… are removed until a review screen exists (V27.7); the rule for presenting Settings over a review stays, for the Sync section.
- **N11.8**, sixth bullet: "The two-collections decision and the publication choice push the chosen direction's preview inside their own sheet" and "This includes the first connection's 'Use iCloud Collection' for an empty collection" are replaced by N15.3 and V27.2. The rest stays: an entry ending in "…" opens a preview and changes nothing, destructive buttons are never the default, and the toolbar placements.
- **N11.8**, bullet 8: the sheet that opens by itself is N15.3's; its Cancel takes the place of the earlier dismiss button.
- **N11.8**, bullets 1, 2, 7, 9 and 10 are unchanged.
- **N12.4**, first bullet: "Account & Sync offers iCloud Sync and Meguri Sync" becomes N15.2's Service row. Second bullet: MP2's native controls and N11.8's safety rules stay; N11.8's iCloud controls are replaced as above. The other bullets are unchanged.
- **Plan text**: R122 (the media setting); R117's Enable and Pause controls; R2046's "explicit connection action" and "a paused connection"; R125's "advanced recovery actions", for their place only (their previews, backups and confirmations stay); R2054's Review Conflicts action in the needs-attention state and Settings, which is met for media by N15.3's sheet together with Show Replaced Media, while conflict copies that are not media have no interface beyond the count (an accepted gap, decided 2026-11-19); U161's Account & Sync layout.
- **Screen contracts**: `settings.accountSync`, `sync.status`, `sync.connect`, `sync.publicationChoice`, `sync.serviceSwitch` and `sync.meguriSetup` need new versions for this section; `sync.reviewConflicts` is not shown; `sync.keepCollection` is new.

## N16. Round 3 review decisions (2026-11-19)

Decided by Antoine in chat on 2026-11-19 (`decisions.md`, the `review.` rows; round 3 options 1F, 2B, 3D, 4D, 5B). Where earlier text differs, this section wins. Kept from 2026-11-01: the white card page, one row of answers with the interval under each name, the row split at the fold on the Duo held wide, the Mac's round Back button, no key hints. The card is the user's template and is never restyled. Buttons and titles keep Apple's title capitalisation ("Show Answer", "Keep Going").

### N16.1 Session progress

- One thin grey bar along the very bottom edge, below the answer buttons, on every device: the system's linear progress indicator with the secondary colour as its tint. No text, no counts, no accent colour.
- Touch: the bar spans the screen's width on its bottom edge, inside the bottom safe area, so the home indicator lies over its middle. iPhone Duo: the same edge folded, held wide and held tall; held wide it runs under both halves and across the fold. It is not a control, so it may cross (R9 covers controls).
- Mac: the bar is the window's last row, under the answer buttons. The Mac had no bar before.
- The bar shows on the question and answer sides and between cards. Loading, waiting, paused and finished states have none.
- Its value is the answers given over those answers plus the cards left (new, learning and to review). Accessibility: label "Session Progress", value "12 of 40", identifier `review.session.progress`. It takes no input and is read as text, so the hit-region audit does not ask a touch target of it.
- Replaces: N11.1's bar across the top and its device setting (R341–R345), N13.3's counts line and top bar.

### N16.2 Sync status during review

- The sync status button is in the review's toolbar only while sync needs the person. Otherwise the toolbar holds Close and Undo (Redo when there is one) on touch, and Back to Decks alone on Mac.
- Sync needs the person when: the provider has stopped or is finishing a provider change; iCloud has a problem to act on, a decision to make or no account it can use (a short-lived failure does not count); Meguri Sync has no sign-in, or waits for a sign-in, a choice of collection or a setup step (signed in with setup not continued counts), or its media, setup or last round stopped and will not retry. iCloud not yet set up on this installation counts as a setup step. The button stays away for transfers, changes not yet sent, no network, failures that retry by themselves and a pause the person chose. With sync off it never shows.
- iPad: the system's sidebar toggle is removed for the whole review, with the sidebar (N11.1); it returns with the deck workspace. It showed beside Close before this round.
- Sync Now during review syncs without presenting the status, which would pause the review, unless sync needs the person. On Mac a request to open Settings is served by the window, so it works while the button is absent.
- The button stays while its own popover or sheet is open, so a state that clears does not close what the person is reading.
- The round 2 split into waiting, needs you and issues (`decisions.md`, `sync.status`) is not built yet; when it is, this rule takes its "needs you" and "issues" kinds.
- Replaces: N3's and N11.1's status icon that stays in the review's corner, N12.3's and N13.3's status toolbar item as a permanent item.

### N16.3 Show Answer and the answer row

- Show Answer is centred in the answer row on touch, as wide as two answer buttons. At the accessibility text sizes it keeps the full row.
- With a fold through the row (iPhone Duo held wide) it keeps the trailing side, over Good and Easy: a centred button would sit on the fold (R9).
- The answer buttons' group is centred: on touch it is the reading column's width (N16.4), on Mac the four standard buttons are centred in it, as is Show Answer.
- Replaces: N13.3's trailing-half Show Answer where no fold crosses the row.

### N16.4 Reading column

- iPad and Mac: the card, any notice above it and the review's states sit in one centred column, `ContentWidth.readable` wide, the width the touch answer row already had. A narrower window uses its full width.
- iPhone and iPhone Duo are unchanged: the card takes the full width in every position. The choice is by idiom, as N13.1's layout choice is.
- The window beside the column shows the card page colour. A template's own background does not reach it yet (N13.4, R335).

### N16.5 End of a session

- A finished session shows the number of cards studied as one large number, "cards in 12 minutes" under it in secondary text, and Done. No symbol, no title, no count per answer, no line about learning cards to come.
- The number counts different cards answered in the session. The time is the session's active study time: whole minutes from one minute up, seconds before that ("card in 40 seconds").
- Keep Going sits beside Done only while learning cards are ready again; the two stack at the accessibility text sizes. Both are standard bordered buttons.
- A session that ends with nothing answered has no number: it says "Finished for Now" over Done.
- N11.6's summary sheet over the deck workspace is unchanged.
- Replaces: the check mark, "Finished for Now" as the finished title and its recorded-answers line.

### N16.6 Waiting, paused and notice states

- Review's states are plain text, centred in N16.4's column: a title, an optional message in secondary text and standard bordered buttons. No symbol.
- Waiting for a learning card that is not ready: "Waiting for Learning", "Your next learning card is not ready yet." and Check Now.
- Paused by an error: "Review Paused", the message and Try Again. Ended: "Session Ended".
- Loading keeps the system progress indicator with its label.
- A notice above the card keeps its warning symbol and text, centred in the column; a long one still scrolls within its limit.

### N16.7 No session timer

- Review has no on-screen timer on any platform, and nothing in Meguri offers one. The timer view, its presentation model, strings and tests are removed.
- The preset's stored fields for showing the timer and stopping it on the answer stay in the collection untouched, for other Anki clients. The app's review bridge no longer reads them.
- Answer time is still measured and saved with each answer, up to the preset's maximum answer seconds.
- Replaces: R364's on-screen timer in N11.1 and N13.3.
- Specs aligned 2026-11-19: `review/timers` T2 and `review/reviewer-strings` S17 are removed, and Deck Options shows neither timer option (`sched/deck-options` O3).

## N17. Round 5 editor and file transfer decisions (2026-11-19)

Decided by Antoine in chat on 2026-11-19 (`decisions.md`, rows `editor.addNote`, `collection.import`, `settings.importExport`; round 5 options 1B, 2B, 3B and the Duo answer). Where earlier text differs, this section wins. Media is never a separate choice in any of these flows.

### N17.1 Add Note

- One grouped form. Its first group has two rows, Deck then Type, each a system menu on its current value (`editor.add.deck`, `editor.add.type`). The deck list follows `editor/add-flow` A2; the deck starts as `notes/add-defaults` gives it. Choosing a type maps the content as A3 says and keeps the deck.
- Each field is a section of that form: its name is the header and one box sits below. The rounded name button and the rounded HTML button on every field are gone. A tap on the header still collapses or expands the field (`editor/field-html` F3).
- The fields stay in one web view, which keeps their scrolling, focus and caret (R5). The Deck and Type group sits above it and does not scroll; it is a real form sized to its two rows.
- One HTML control for the focused field, the `chevron.left.forwardslash.chevron.right` symbol labelled "HTML Editor", selected while that field shows its HTML. Touch: at the leading end of the keyboard bar, with Previous Field, Next Field and Done; the bar is a system toolbar that replaces WebKit's own. Mac: at the leading end of the panel's bottom bar, opposite Add. ⇧⌘X stays. The control takes the identifier of the field it acts on (`editor.field.<ord>.htmlToggle`), and `editor.add.htmlToggle` while no field has had focus.
- Add is disabled until the first field has content: typing counts at once, and a committed value follows the live field check. The live "Enter content in the first field." line is gone. The line still names a refused add (`editor/add-flow` A5 step 4), which now needs a race to reach.
- After an add the fields clear (sticky values stay), focus returns to the first field, and the navigation bar's subtitle reads "Added to <deck>", the deck's last name component; on Mac it sits beside the panel's title (`editor.add.result`). It covers no field. VoiceOver announces it. It stays until the next note is started, the selection changes or the sheet closes. The sheet stays open.
- iPhone Duo opened wide, and any iPhone at regular width: Add Note covers the whole screen, with the form in a centred column no wider than the readable width. Folded or held tall: the sheet. iPad: the system page sheet. A fold or unfold while it is open moves the same draft and web view to the other presentation.
- Mac: the trailing panel keeps its title bar. Deck and Type are a grouped form; under it each field has its name in a leading, right-aligned column beside its box. ⌘↩ adds.
- The next draft loads into the page that is already showing, so on touch the keyboard stays up and the caret is in the first field. A hold refuses input without taking focus from the field.
- Not built, and why: Return in the Mac's last field does not add, because `editor/field-html` H4 gives Return to the field (a new line). The confirmation has no card count and no Undo (`editor/add-flow` A5 step 6 names both). Changing type keeps the deck; `editor/config-keys` K4.2's deck for the type is not read yet.
- Specs aligned 2026-11-19: `editor/add-flow` A5.1 now gives the disabled Add, the refused add's line and the "Added to <deck>" confirmation with no count and no Undo; `editor/field-html` F1 gives the one HTML control. The "Not built" notes above on the confirmation therefore no longer name a difference from the spec; Return in the last field and K4.2 stand as written.
- Replaces, in `screen-inventory.yaml`'s `editor.add`: Type before Deck as text rows, "Empty: … Add enabled" and the "1 card added" toast wording. Replaces, in `editor/field-html` F1: a separate mode button on every field.

### N17.2 Importing a collection file

- Import (File › Import…, Settings, the empty collection) opens the system file picker at once, on the window that asked, limited to collection packages. No sheet comes first.
- A chosen file opens one sheet. While the file is read it shows "Reading File…" with Cancel. Then the confirmation: `Import “<file name>”?`, one summary line "N notes · N cards · N media files" with plural forms, and, only when a collection is replaced, "This replaces the collection on this iPhone. Meguri keeps a backup of the current one." (iPad, Mac). Import and Cancel are the two actions.
- A footnote line under the summary, in the same row, says "Its media (<size>) is merged with the media on this iPhone; files already here are kept." It shows only when the file has media. `package/container` P16.4 asks for the size and for both statements; this is their one line.
- The summary's accessibility value gives the review count, which the line itself leaves out.
- After Import the sheet shows "Importing…" with Cancel while cancelling is safe, then closes on success. Nothing in the import changed: inspection, the safety backup and recovery run as before.
- A refused file shows one plain message with Choose Another File. Recovery states keep their message and the receipts the person must keep (backup, retained media reference); those rows appear only there.
- Removed from the interface: the explanation sheet with Choose File, the Collection Preview header, the Destination, Validated media files and Decoded media bytes rows, the two paragraphs, the media plan rows during progress and after an ordinary cancel, and the "Replace Collection" label.
- Specs aligned 2026-11-19: `package/transfer-sheets` X7 states this flow, and `package/container` P16.4 no longer requires the decoded size or the merged and kept media statements in the interface. The footnote line above is therefore optional; the merge itself is unchanged.
- `package/transfer-sheets` X3 wins over the sketch for the button: when a collection is replaced, Import has the destructive role and is not the default, so Return confirms nothing. With no collection to replace, Import is the prominent default action.

### N17.3 Exporting the collection

- Export Collection starts at once: the sheet shows "Preparing Export…" with a progress bar (a spinner while no total is known) and Cancel. When the file is ready the system save dialog opens over it: the Files export on touch, the save panel on Mac.
- Cancel while preparing, and Cancel in the save dialog, discard the prepared file and close the sheet. Cancel is disabled while the save dialog shows and while the file is being saved. No route leaves the prepared file or the provider's hold on it behind a closed sheet. A finished save does the same, with no "Saved" step.
- A save that fails keeps the sheet with one message, Choose Another Location and Cancel; the file is kept for that retry (`package/file-export` E4) and discarded on Cancel. An interrupted export's leftover file is discarded when the next export starts.
- Settings › Collection: the footer under the Export row reads "Export saves all decks, cards, review history and media in one file." The export's contents did not change.
- Removed: Prepare Export, Choose Save Location, Discard Prepared Export, Done, "Ready to Save" and the explanation paragraphs.
- Replaces: N9's "Keep the current pre-export explanation" and the prepared file that outlived its sheet.
- Specs aligned 2026-11-19: `package/transfer-sheets` X7.3, with `package/file-export` E1 and E4.

## N18. Round 4 first-run decisions (2026-11-19)

Decided by Antoine in chat on 2026-11-19 (`decisions.md`, the two `home.fsrsSheet` rows; the `onboarding.welcome` row; round 4 option 1B). Where earlier text differs, this section wins. The behaviour of N18.1 is `sched/fsrs-policy` A1 and `sched/app-coordination` F5.

### N18.1 FSRS is always on

- No FSRS sheet, prompt, notice, setting, menu command or option exists on any platform. The screen `home.fsrsSheet` is removed, with its view, strings and snapshots.
- A collection that opens with FSRS off opens as usual, and Meguri turns FSRS on in the background: at once with sync Off, Sync Automatically off or a service that needs the person; otherwise after a collection exchange, or when study is started (`sched/app-coordination` F5.1; decided 2026-11-19 (review F2)). Nothing about it is shown and nothing is asked. Study entry points show their normal loading state until it is done; a study entry point never waits for the network.
- When that step fails, the collection stays open: study is refused with "Meguri couldn’t prepare this collection for study." (`study.prepare.failed`) and Try Again at the study entry points, while browsing, adding notes, import, export and sync stay available. A failure at open and a later one behave the same. No route studies with FSRS off.
- Accepted deviation from `sched/app-coordination` F5.3's "one Try Again button" (review of the FSRS unit, 2026-11-20): at a Study control the sentence is a system alert with Try Again and Cancel, because an alert needs a way to dismiss it; Cancel changes nothing and study stays refused. In Go to Deck the sentence shows in the sheet and Go is the control that tries again. While the step runs, Go to Deck waits for it and then opens study.
- A step that the core refuses or stops for other work on the collection (an export, an import, a replacement, the app moving to the background) is not a failure: nothing is shown, and the next sync or study entry point runs it.
- When another client turns FSRS off, Meguri turns it on again each time and says nothing; each time is written to the diagnostic log.
- The change syncs like any other change. Its risks for the owner are in `sched/fsrs-policy` A1.6.
- Replaces: R702's and R703's sheet, U51's `FsrsEnabledSheet`, AE29's "after the one-time sheet".

### N18.2 Welcome

- With no collection, the root content is the welcome screen (`onboarding.welcome`), as before: no sidebar, tab bar or sync button.
- Its title block: the app icon, the title "Welcome to Meguri" and one line of secondary text. Its choices: three, in this order, each a title with one line of explanation under it in secondary text:
  1. **Set Up Sync**: "Use Meguri Sync or iCloud to bring your collection to this device and keep your devices in step."
  2. **Import a Collection File**: "Open a collection file exported from Meguri or Anki."
  3. **Start Empty**: "Begin with a new, empty collection on this device."
- The explanations and the title block's line are spec-writer wording (2026-11-19), open to Antoine's edit; the three titles are decided. Apple's title capitalisation (N16).
- The choices are rows of one system inset grouped list, each a button over its whole row with the system's disclosure indicator. No choice is prominent or the default; Return activates the focused row. No symbol is required. The four equal buttons, and the two that led to the same sync flow, are gone.
- **Set Up Sync** presents N15.2's Sync section in a sheet, with its Service control at Off. Choosing a service there is the consent to set it up (V27.2 rows 1 and 2, MP6.3 row 1); the sheet closes when a collection is open, and Cancel returns to the welcome screen with the service Off. With iCloud chosen and no collection anywhere, Meguri creates an empty collection and publishes it, and the sheet closes onto the empty deck list (V27.2 row 1; decided 2026-11-19 (review F6)). The row is never disabled for an iCloud account state: that state shows in the Sync section, where Meguri Sync can still be chosen. iCloud carries its Beta label there (N15.2).
- **Import a Collection File** opens the system file picker at once (N17.2). With no collection to replace, the confirmation's Import is the prominent default.
- **Start Empty** creates the collection with the Default deck and the stock note types (R161), local only, and opens it; N18.1 applies with no prompt. A failure shows its cause in one line on the welcome screen, which stays.
- iPhone, and the iPhone Duo folded or held tall: the title block above the choices, in one scrolling column. iPad and Mac: the same order in one centred column no wider than the readable width; the Mac window shows no clipped control at its minimum size.
- iPhone Duo opened wide: the title block is centred in one half and the choices' list fills the other; nothing sits on or crosses the fold (R9). The sheets and the file picker that a choice opens follow their own rules for that position.
- At the accessibility text sizes each row's explanation wraps under its title, and the screen scrolls. VoiceOver: the title is a header; each row reads its title, then its explanation, as one button.
- String Catalog keys (decided 2026-11-19 (review F12)): `onboarding.welcome.title`, `onboarding.welcome.message`, and for each choice its title and explanation: `onboarding.welcome.sync.title`, `.sync.detail`, `onboarding.welcome.import.title`, `.import.detail`, `onboarding.welcome.startEmpty.title`, `.startEmpty.detail`. The explanation lines are accepted as specified.
- Identifiers: `onboarding.welcome.sync`, `onboarding.welcome.import`, `onboarding.welcome.startEmpty`. `onboarding.welcome.connect` is retired.
- Mac: Meguri › Settings… and Quit stay available. On iPhone and iPad Settings is not reachable from the welcome screen except through Set Up Sync's sheet.
- Until N15 is built, Set Up Sync opens the sync settings that exist.
- Replaces: R157's and R1591's "Connect to iCloud" as the first choice and its disabled state with a reason line; the `onboarding.welcome` screen contract's three buttons, its prominent style rules and its line about private iCloud; the empty state's separate sync buttons. `sync/status-strings` T12's `sync.connect.action` is no longer shown here (T19.8).

### N18.3 Welcome screen

- Home with no collection shows a welcome in place of the "No Collection" state and its four equal buttons: the large title "Welcome to Meguri", the secondary line "Choose how to start. You can change this later in Settings.", and one system grouped list of three rows.
- Each row has a leading symbol, a title, one line of explanation and a trailing disclosure chevron, in this order:
  1. Set Up Sync (`cloud`): "Get your collection from Meguri Sync or iCloud." It opens Account & Sync (`home.collection.accountSync`): the sheet on touch, the Settings window on Mac. The separate Connect to iCloud… button and its reason line are gone; Account & Sync says why a service is unavailable.
  2. Import a Collection File (`folder`): "Open a .colpkg file exported from Meguri or Anki." It opens the file picker (N17.2; `home.collection.import`).
  3. Start Empty (`plus`): "Begin with no decks and add your own notes." (`home.collection.startEmpty`).
- A row is disabled only where its button was: Set Up Sync and Import while another sheet or flow owns the window, Start Empty while it cannot start. A disabled row stays visible.
- The rows are buttons in a grouped form, at least the minimum target high. The chevron is drawn, because a row that acts in place gets none from the system. Title and explanation wrap at every text size. At the accessibility sizes the symbol and the chevron are left out and the text takes the row, as the state views leave out their symbols: a label there wraps each text around its symbol. VoiceOver reads the title as the row's label and the explanation as its hint; the title of the screen is a header.
- Compact width: the title and line lead the list as its header, at the top of the page. In the tab layout the navigation bar has no title here: the welcome's own title is the page's header.
- iPad and Mac: the same list in a centred column of the readable width in the detail area, from the top. The page is still titled "Today" where the sidebar's first row says so (N14.1).
- iPhone at regular width, wider than tall and at least 480 points high, below the accessibility text sizes (the Duo unfolded and held wide): no list beside a detail. A large iPhone in landscape is too short for it and keeps the column, as the accessibility sizes do. The title and line are vertically centred on the leading half and the three rows on the trailing half, either side of the fold, which the layout reads as the answer row does (N13.1); without a fold in the view, two equal halves. A form has no vertical alignment of its own, so the rows' measured height sets its top margin. Rows that leave less than 48 points above and below, or that an error or the previous session follows, keep the system's margin and scroll from the top: the margin never follows scrolling.
- While the collection is still opening, the tab layout keeps the arrangement it last showed and starts a launch on the single column, so no list beside a detail shows for a moment before the welcome.
- On touch the navigation bar over the welcome is opaque, in the page's grouped background colour: nothing shows at rest, and rows that scroll under it are covered. Under the system's translucent bar the contrast audit failed a row title seen through the bar, at the largest text size in dark.
- An error from the last attempt and the previous session's line follow the rows, each in its own group.
- Start Empty, the import and sync themselves did not change.
- Replaces: N2's centred "No Collection" state for this page, `workflow.md`'s empty-collection Import button as a button, and `screen-inventory.yaml`'s `onboarding.welcome` layout (app icon, three bordered buttons, the reason line under Connect to iCloud… and its identifiers `onboarding.welcome.*`).

## N20. Round 6 decisions (2026-11-20)

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, rows `home.goToDeck`, `home.deckActions`, `home.deckName`, `shell.commands`; round 6 options 1C, 2C, 3B, 5B). Where earlier text differs, this section wins. It covers the screens that exist. Delete Deck (4B), Deck Options (6B) and Edit Note (7A) arrive with their units, which add their own subsections; their places below are kept for them. The name rules of N20.3 are `decks/decks` D19, the create row's D18. Meguri authors these strings; implementers take them from N20.6.

### N20.1 Deck search in the sidebar

- Sidebar layouts (Mac, iPad at regular width): a search field at the top of the sidebar, in the system's sidebar placement for `.searchable`, with the prompt "Search or Create a Deck". It replaces the Go to Deck sheet.
- Typing replaces the sidebar's list, the Today row included, with the results, as System Settings does. Each result is two lines: the deck's last name component over its parents in secondary text, joined with " › " (a top-level deck has no second line). Matching and order are the touch Search's (N14.2): `decks/deck-list` H6.1 and H6.2. Filtered decks are listed.
- After the results, while `decks/decks` D18 finds no deck with the typed name: a row with the `plus` symbol, "Create deck named “<query>”". It opens N20.3's New Deck form with the query as the Name and Parent Deck None.
- With no result and no create row, H6.3's one-line message shows.
- Return or a click opens a result: its deck page, with the sidebar selection on that deck; a filtered deck shows R53's explanation (`decks/deck-list` H3). The query and results stay while it is open, and the opened deck shows selected among them. Esc clears the field and the tree returns, the opened deck still selected.
- Mac: after each edit the first result is highlighted, or the create row when nothing is listed; ↑ and ↓ move the highlight over all rows and wrap (H6.3). The highlight does not change the detail; Return opens the highlighted row. iPad: a tap opens a result; Return in the field opens the first.
- ⌘F (Edit › Find…), ⇧⌘O (a second key of Find…, with no menu item of its own) and "/" in the `decks.list` and `decks.page` contexts (H9.2; never while a text field or other view owns the key, N4) put the focus in the field, showing the sidebar first when it is hidden. In the tab layout the same keys present the deck list's Search (N14.2). During review Find… is disabled.
- Sidebar layouts' toolbar, on Today and the deck page: the sync status button and one Add menu (`plus`) with New Note (`square.and.pencil`) and New Deck… (`folder.badge.plus`). No separate Add Note, Go to Deck or Create Deck button. On a deck page New Note presets that deck, as Add Note does now (`notes/add-defaults`).
- "New Deck…" is the one name of that action in every menu; its form is titled "New Deck". "Create Deck" is no longer used.
- The tab layout keeps N14.2's Search button and Add menu, unchanged.
- The Go to Deck sheet is retired on every platform: no button, menu item or key opens it. Every list of deck search results shows the two-line rows, never a `::` path, and opens the deck page in every layout (N20.5).
- Replaces: N14.2's "The Go to Deck sheet stays for the sidebar layouts' toolbar, the "/" key and the Mac's Deck menu" and its "opens the Create Deck sheet" (now the New Deck form); N14.3's "Sidebar layouts keep Go to Deck and Create Deck in the window toolbar"; R26's sheet (its filter stays, H6.2); `decks/deck-list` H6.3's sheet, its highlight on opening and its Return that starts review, and H9.2's `/` and ⇧⌘O rows (now Find…); N18.1's Go to Deck sentence, which no longer has a screen.

### N20.2 The deck menu

- One menu per deck row: right-click on Mac, long press on touch. Its groups, in order:

| Group | Item | Symbol | Mac key | Filtered deck |
|---|---|---|---|---|
| 1 | Study | `play` | ⌘R | absent |
| 1 | Add Note | `square.and.pencil` | | absent |
| 2 | Rename… | `pencil` | ⇧⌘R | shown |
| 2 | New Subdeck… | `folder.badge.plus` | | absent |
| 2 | Deck Options… (N20.9) | `gearshape` | ⌥⌘O | absent |
| 3 | Delete Deck (destructive role, N20.7) | `trash` | ⌘⌫ | shown |

- A filtered deck's menu holds Rename… and Delete Deck (R53).
- Study is disabled under N14.3's condition, through the same availability function. Add Note opens `editor.add` with that deck (`notes/add-defaults`). Rename… and New Subdeck… open N20.3's form.
- Overview and the Mac row's hover ⋯ button ("Deck Actions") are removed; a tap or click on the row opens the deck page.
- Mac: the menu shows the Deck menu's keys beside its items (N20.4). Touch menus show no keys.
- iPhone Duo held wide: the menu opens beside the row in the list's half, as the system places it; nothing sits on the fold.
- Replaces: R47's items and order for this menu (an R47 item not listed here comes back only with the unit that brings it, which says where); N14.3's "The long-press deck menu is otherwise unchanged"; `decks/deck-list` H3's "the row menu's Overview opens the deck page"; `home.deckMenu`'s hover ⋯ and its menu of Study and Overview.

### N20.3 Naming a deck

- New Deck…, New Subdeck… and Rename… use one system `Form` with two rows:
  1. **Name**: a text field labelled "Name", with focus and the cursor at the end when the form opens.
  2. **Parent Deck**: a menu-style `Picker` labelled "Parent Deck". Its items: "None" (top level), a separator, then D19.2's decks in tree order, each its last name component, indented one level per depth. Each item's accessibility label is its full path joined with " › ". The row shows the chosen deck's last name component; its accessibility value is the full path.
- Title and confirm button: New Deck… gives "New Deck" and Create; New Subdeck… gives "New Subdeck" and Create; Rename… gives "Rename Deck" and Rename. Cancel is the cancellation action, the confirm button the confirmation action. Defaults and when the confirm button is enabled: D19.3 and D19.4.
- Footer (D19.6): Rename of a deck with subdecks: "Its subdeck moves with it." or "Its N subdecks move with it.". Otherwise, with a parent: "Inside <parent path>.", the path joined with " › ". With None: no footer.
- Confirming composes the full name and runs the create or rename of D19.5. A refusal shows in one line under the form, which stays with the draft. The existing-name rule and the final-name report are `decks/deck-list` H7's. A created deck opens its deck page in every layout.
- Meguri's interface never shows the `::` notation: a deck path in a label, help tag, accessibility label or value joins the components with " › ". A `::` typed in the Name still creates nested decks.
- Presentation:

| Platform | The form |
|---|---|
| iPhone; iPhone Duo folded or held tall | a sheet |
| iPhone Duo held wide (iPhone at regular width with the list beside the detail, N14.10) | in the detail half, in place of what it showed, with Cancel and the confirm button in that half's navigation bar; no field crosses the fold. Cancel returns the detail to what it showed; the confirm button shows the deck's page. Choosing another row in the list closes the form as Cancel does |
| iPad | a sheet |
| Mac | a window sheet; the form keeps the macOS default style, with leading labels |

- Replaces: R59's and R60's pre-filled paths and R60's "typing a different path moves the deck" (a changed Parent Deck moves it; R58 stays); `decks/deck-list` H7's Rename and New Subdeck pre-fills and its untouched-pre-fill rule (D19); `home.deckName`'s single name field and "Deck name" prompt; the `::` form of the full path in N2's and N11.5's hover help and accessibility text (now joined with " › ").

### N20.4 The menu bar by object

The menu bar, in order: Meguri, File, Edit, View, Deck, Card, Window, Help. The Study menu is removed: there is one Undo and one Redo, in Edit. The Go to Deck… item is removed.

| Menu | Item | Key | Enabled when |
|---|---|---|---|
| File | New Note | ⌘N | a collection is open, outside review, and no sheet or panel owns the keys. Opens `editor.add`, with the deck preset on a deck page. Disabled during review until N11.1's review-time Add Note panel exists (deferred) |
| File | New Deck… | ⇧⌘N | a collection is open, outside review; opens N20.3's New Deck form |
| File | Import Collection… | ⇧⌘I | as N17.2 |
| File | Export Collection… | ⇧⌘E | as N17.3 |
| File | Sync Now | ⇧⌘Y | `syncNowAvailability()` (N11.8) |
| File | Close | ⌘W | the system's |
| Edit | Undo <step name>, Redo <step name> | ⌘Z, ⇧⌘Z | titled with the core's undo step name ("Undo Rename Deck", "Undo Answer Card"), the review's existing pattern, until `review/reviewer-strings` S5's catalog keys exist (R173); plain "Undo" and "Redo", disabled, when nothing can be undone or redone. A text input that has focus keeps its own undo; otherwise they act on the collection's undo history, in review and outside it |
| Edit | Cut, Copy, Paste | ⌘X, ⌘C, ⌘V | the system's |
| Edit | Find… | ⌘F; also ⇧⌘O | the deck workspace shows and no sheet or panel owns the keys; focuses N20.1's field (tab layout: presents the Search) |
| Deck | Study | ⌘R | the shown deck's Study is available (N14.3); never for a filtered deck (H3) |
| Deck | Rename… | ⇧⌘R | a deck is shown, filtered included |
| Deck | New Subdeck… | | a normal deck is shown |
| Deck | Delete Deck | ⌘⌫ | a deck is shown, filtered included, outside review, and no text field, sheet or panel owns the keys (N4, H9.2); N20.7 |
| Deck | Deck Options… | ⌥⌘O | a normal deck is shown, outside review; N20.9 |
| Card | Show Answer | Space | review shows a question side |
| Card | Again, Hard, Good, Easy | 1, 2, 3, 4 | review shows an answer side |
| Card | Back to Decks | Esc | during review |
| Card | Edit Note | ⌘E | a card is presented, on either side; N20.8 |

- Separators: File after New Deck…, Export Collection… and Sync Now; Edit after Redo and Paste; Deck after Study, New Subdeck… and Delete Deck; Card after Show Answer, Easy and Back to Decks. A separator with nothing after it yet is left out.
- The Deck menu acts on the deck the workspace shows: the sidebar's selection (Mac, iPad), or the deck page shown (tab layout). With none (Today, the welcome, review) its items are disabled.
- Each command follows N4: disabled outside its context, routed through the action and guard of its visible control, and subject to input ownership. The Card menu's unmodified keys follow N11.3: the card page keeps the first claim. ⌘⌫ and the other menu key equivalents never take a key from a text input (H9.2).
- iPad with a keyboard: the same menus, items and keys in the iPad menu bar and in the list the system shows while ⌘ is held (SwiftUI `commands`). iPhone and the iPhone Duo have no menu bar: with a keyboard the same keys act, and where the system shows its ⌘-held list it follows these menus.
- Replaces: N11.3's first bullet's Study menu, and the Undo Answer and Redo Answer items it named; R280's New Deck… ⌥⌘N, Export… ⌘E (⌘E becomes Edit Note's) and the labels "Import…" and "Export…"; R282's ⇧⌘N for Note Types (the key is New Deck…'s; Note Types' key is open); R283's Go to Deck… (⇧⌘O is Find…'s second key), its S and Return as Study's menu keys (S and Return stay deck-list keys, H9.2) and "Delete Deck…" (now "Delete Deck"); the current Deck menu's "Rename Deck…" (now "Rename…").

### N20.5 Consistency fixes

1. Every list of deck search results, on touch and in the sidebar, shows each deck as its name over its parent path joined with " › " (N14.2, N20.1), never a raw `A::B::C` path.
2. The touch review's close button (N11.1, N13.3): its title, accessibility label, Voice Control name and help are "Back to Decks". "Close Review" is no longer used. The Mac's round Back button is unchanged (N12.3).
3. Activating a deck search result (Return, a tap or a click) opens its deck page in the compact and sidebar layouts alike, and never starts review.

- Replaces: R27's "starts its review (iPhone layout)" and `decks/deck-list` H6.3's same rule; R284's "Close Review" (N11.3 already named it).

### N20.6 Strings

The English text of the screens above. "New" marks text not used before this round; the others stay as they are. Units that land later own the text marked "with its unit".

| Text | Where | |
|---|---|---|
| Search or Create a Deck | the sidebar search prompt (also N14.2's) | |
| Create deck named “%@” | the search's last row; %@ is the typed text, between U+201C and U+201D | changed: the earlier text had no quotation marks |
| ` › ` (space, U+203A, space) | between the components of every shown deck path | |
| New Note | Add menu, File menu | |
| New Deck… | Add menu, File menu | |
| New Deck, New Subdeck, Rename Deck | the form's titles | New Deck new |
| Name, Parent Deck | the form's row labels | new |
| None | Parent Deck's first item | new |
| Create, Rename, Cancel | the form's buttons | |
| Its subdeck moves with it. | footer, one subdeck | new |
| Its %lld subdecks move with it. | footer, other counts; a String Catalog plural with the one form above | new |
| Inside %@. | footer; %@ is the parent path joined with " › " | new |
| Study, Add Note, Rename…, New Subdeck… | the deck menu; Rename…, New Subdeck… and Study in the Deck menu | Rename… new in the Deck menu |
| Delete Deck | the deck menu, the Deck menu, the touch confirmation's button | new (N20.7) |
| Deck, Card | top-level menu titles | Card new |
| Import Collection…, Export Collection…, Sync Now, Close | File menu | |
| Undo, Redo, Undo <step name>, Redo <step name> | Edit menu; <step name> is the core's undo step name until S5's keys exist (R173) | |
| Find… | Edit menu | new |
| Deck Options… | the Deck menu, the deck menu | N20.9 |
| Show Answer, Again, Hard, Good, Easy, Back to Decks | Card menu; Back to Decks also the touch close button | |
| Edit Note | the Card menu, the card's context menu (Mac), the touch review toolbar button, the form's title | N20.8 |

- No longer used: "Go to Deck…", "Create Deck", "Overview", "Deck Actions", "Deck name", "Rename Deck…", "Close Review", the Study menu's title and its "Undo Answer" and "Redo Answer" items (Edit's Undo still reads "Undo Answer Card" after an answer, from the core's step name), "Import…", "Export…".

### N20.7 Deleting a deck

Decided 2026-11-20 (`decisions.md`, `home.deckActions`, option 4B). The operation is `decks/decks` D8; its summary D8.5.

- Entries: the deck menu's Delete Deck (N20.2, third group, destructive role, `trash`; filtered decks too) and Deck › Delete Deck (⌘⌫, N20.4), both on that one deck. No other entry; no swipe action (R49).
- Delete Deck first reads D8.5's summary for the deck. With `cards` 0 the deck is deleted at once, with no question: an empty deck, and a filtered deck, whose cards return home (D9). With `cards` 1 or more Meguri asks first. When the collection changes between the read and the delete (a new generation), the summary is read again before anything is deleted.
- The question: the title `Delete “<name>”?` with the deck's last name component, and one message line from the subdecks (S = `decks` − 1, the deck's descendants) and C = `cards`:

| S | C | Message |
|---|---|---|
| 0 | 1 | Its 1 card will also be deleted. |
| 0 | 2 or more | Its C cards will also be deleted. |
| 1 | 1 | Its subdeck and 1 card will also be deleted. |
| 1 | 2 or more | Its subdeck and C cards will also be deleted. |
| 2 or more | 1 | Its S subdecks and 1 card will also be deleted. |
| 2 or more | 2 or more | Its S subdecks and C cards will also be deleted. |

  Numbers use the locale's grouping ("412", "1,204").
- The system's confirmation, in each place:

| Where | Form | Buttons |
|---|---|---|
| iPhone; iPhone Duo folded or held tall; iPad at compact width | a confirmation dialog from the bottom | Delete Deck (destructive), Cancel |
| Regular width on touch (iPad, the Duo held wide) | a popover beside the deck's row, in the list's half; from the menu bar with no row in view, the system's placement | Delete Deck (destructive); tapping elsewhere cancels, so there is no Cancel |
| Mac | an alert on the window, with the app icon | Cancel and Delete; Delete has the destructive role and is never the default, so Return deletes nothing; Esc cancels |

- Cancelling changes nothing.
- Deleting is one undo step, "Delete Deck" (D8 step 2): Edit › Undo Delete Deck (R173) on Mac and on iPad's menu bar restores the deck, its subdecks and cards; on iPhone, shake to undo where the system offers it. No banner and no toast.
- When the deck shown in the detail, or the deck page on the stack, is one of the deleted decks, the workspace returns to its root: Today in the sidebar layouts and on the Duo held wide; the deck list in the tab layout. N14.1's rule for a deck that leaves the collection covers it. Otherwise the detail stays.
- The default deck (D8, R68): Delete Deck on it removes its cards and subdecks and keeps the deck, renamed "Default" and moved to the top level. It asks as any deck does. Afterwards it is an empty deck: D12 hides it when another top-level deck exists, otherwise its row shows as "Default" with no count (AE1). The workspace returns to its root as above, since the deck shown was emptied.
- A delete that takes the backup of D8 step 4 shows nothing extra. A delete that fails writes nothing and shows an alert: the title `Couldn’t Delete “<name>”`, the message "Meguri couldn’t make a backup first, so nothing was deleted." for `SafetyBackupFailed` and "Nothing was deleted." for any other failure, and OK.
- While the delete runs, the deck's menu items and the Deck menu are disabled.
- Strings, all new: `Delete “%@”?`, the six messages above (a String Catalog entry with plural variations for S and C), "Delete Deck", "Delete" (Mac alert), "Cancel", `Couldn’t Delete “%@”`, "Meguri couldn’t make a backup first, so nothing was deleted.", "Nothing was deleted.", "OK", and Edit's "Undo Delete Deck" and "Redo Delete Deck" (R173's pattern).
- Replaces: R66's toast "Deleted <deck> (N cards)" with Undo (no toast); `home.deckMenu`'s "deletes … end with an Undo toast"; R67 stays as D8.5 states it.

### N20.8 Edit Note during a review

Decided 2026-11-20 (`decisions.md`, `review.editNote`, option 7A). The review side is `review/edit-current`; the card browser is not part of this round.

- Touch: the review toolbar gains an Edit Note button, the `pencil` symbol labelled "Edit Note", after Undo (and Redo when there is one) and before the sync status button (N16.2). Mac: Card › Edit Note (⌘E) and the card's context menu; no toolbar button, as with Undo (N11.1). The E key opens it on every platform (`review/commands`). All are enabled while a card is presented, on either side, and disabled in the waiting, paused, finished and loading states.
- It opens round 5's Add Note form (N17.1) on the current card's note, titled "Edit Note", with Cancel and Done. Its first group, Deck and Type, is replaced by the navigation bar's subtitle "<deck> · <note type>": the card's deck by its last name component, and the note type's name. Neither can be changed here. The fields, the HTML control and the tags are N17.1's; the first field has focus.
- Placement, as Add Note's: iPhone, and the iPhone Duo folded or held tall, a sheet; the Duo held wide (any iPhone at regular width), the whole screen with the form in a centred column of the readable width; iPad, the system page sheet; Mac, a panel over the card, as Add Note's panel during a review, which owns the keys (N11.1). A fold or unfold moves the same draft to the other presentation.
- Changes are a draft. Done saves them with one note update (`review/edit-current#E2`) and closes; the card is shown again, re-rendered, at the same place in the session, with its timing and undo history kept (E3). Done with no change closes. Cancel, Esc, ⌘W and the swipe down close without saving; with a change they first ask "Discard changes?" with Discard (destructive) and Keep Editing (`editor/editing-strings`, the wording of the editors that save on confirm). ⌘Return is Done on Mac.
- Later, not now: when Bury, Suspend and Flag arrive, the pencil becomes a ⋯ menu that holds Edit Note first.
- Strings: "Edit Note" (exists, `review.menu.editNote`), "<deck> · <note type>" with " · " (space, U+00B7, space) between them (new), "Done", "Cancel", "Discard changes?", "Discard", "Keep Editing" (all existing).
- Replaces: R394's iPad "trailing inspector" and its panel as U190's editor in edit mode; R397's Close button (now Cancel and Done); R824 and R863's Esc that saves, for this form (`editor/edit-flows` W3 now names Browse's pane only); the touch review toolbar of N16.2 ("Close and Undo"), which gains this button; `review.editCurrent`'s purpose.

### N20.9 Deck Options, first version

Decided 2026-11-20 (`decisions.md`, `deck.options`, option 6B). The scope, the save input and the strings are `sched/deck-options` O17 and `decks/deck-options-strings` K10. The Advanced page comes later and does not change this page.

- Entries: the deck menu's Deck Options… (`gearshape`, group 2 after New Subdeck…, normal decks only; N20.2), Deck › Deck Options… (⌥⌘O, N20.4), the O key on the deck list and the deck page (`decks/deck-list` H9.2) and the finished state's Deck Options link (H5.2). Not offered during a review in this version: the Deck menu is disabled then (N20.4), and the review menu's item comes with the review menu.
- One grouped form, three sections:
  1. no header: **Preset**, a menu picker of the collection's presets, each listed as `decks/deck-options-strings` K6 gives it ("Default (used by 6 decks)"); the row shows the chosen preset's name. Footer: "Used by N decks. Changes apply to all of them.", or "Used by this deck only." when N is 1, with N the count the picker shows (`sched/deck-options` O6.1).
  2. **Daily Limits**: New Cards per Day, Maximum Reviews per Day, each a number field trailing in its row.
  3. **Memory**: Desired Retention, a percentage field with a stepper of 1 %. Footer: "How likely you are to remember a card when it comes back."
- When the deck holds its own value for a row (a This deck or active Today only limit, or its own desired retention; O2 item 3), the section's footer adds "This deck has its own limits, which stay in force." (Daily Limits) or "This deck has its own desired retention, which stays in force." (Memory). The page cannot change or remove them in this version.
- No limit tabs, no Advanced row, no FSRS switch (N18.1), no preset Add, Clone, Rename or Remove, no Save to All Subdecks, no restore-default or help controls.
- Issues: O4's errors and warnings of the three rows show under their row as N11.9 says; an error disables Save.
- Placement, pushed like a page: compact (iPhone, the Duo folded or held tall, iPad at compact width) full screen with a back button; the Duo held wide, iPad and Mac in the detail area in place of the deck page. Touch: the navigation title "Deck Options" and Save as the confirmation action. Mac: the toolbar title "Deck Options" with the deck's path joined with " › " as its subtitle, the back button, and Save as the toolbar's confirmation (⌘S; ⌘↩ too, O15.3).
- Save runs O17's save; on success the page returns to the deck page. A refused save keeps the page with its issues.
- Leaving: while the draft differs (O6.6), Cancel replaces the back button and swipe back is off on every platform (N11.9's iPhone rule); Cancel, Esc (Mac) and choosing another sidebar row ask "Discard changes?" with Discard (destructive) and Keep Editing. With no change, back leaves at once.
- Strings (new unless marked): "Deck Options" (title; the undo step's name exists), "Deck Options…" (exists in R283), "Preset" (K4's tab text, here a row label), "Daily Limits" (K2), "New Cards per Day", "Maximum Reviews per Day", "Memory", "Desired Retention", "Used by %@ decks. Changes apply to all of them.", "Used by this deck only.", "How likely you are to remember a card when it comes back.", "This deck has its own limits, which stay in force.", "This deck has its own desired retention, which stays in force.", "Save", and the existing "Discard changes?", "Discard", "Keep Editing", "Cancel". Keys: K10.
- Replaces, for this version: N11.9's Preset Actions menu, Save to All Subdecks, restore-default and help buttons, the reload row, two columns and the review-time sheet (each comes back with the unit that brings its part); R736's Anki labels for these three options (6B: Apple title case, K10); R740's and R742's tabs; the screen contract `deckOptions.options`' layout until the Advanced page lands.
