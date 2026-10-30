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
- Keep a quiet transfer-state icon at the upper right. Its popover explains the real recorded state.
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
- Upper right, leading to trailing: the timer when enabled (R364); on iPhone and iPad the undo button (U179) and the ⋯ menu button (U182); then the sync status icon (N3; `sync.status`'s button, whose states that contract gives, N11.8). The Mac has no on-screen undo or ⋯: undo and redo are in the Edit menu, card actions in the Card menu and the card's context menu (R288).
- The progress bar (R341–R345, when its device setting is on) is a thin native linear progress indicator across the top of the review area, below the corner controls.
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
