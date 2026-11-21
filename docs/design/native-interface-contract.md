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
- Upper right, leading to trailing: on iPhone and iPad the undo button (U179) and the More button (U182; the `ellipsis` symbol labelled "More", whose menu N20.8 gives; no toasts); then the sync status icon, only while sync needs the person (N16.2; N3; `sync.status`'s button, whose states that contract gives, N11.8). The Mac has no on-screen undo or ⋯: undo and redo are in the Edit menu, card actions in the Card menu and the card's context menu (R288).
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

Replaced 2026-11-20 by N22 (round 9), approved by Antoine that day: the Note Types section, one draft and one Save sheet. The text below describes the screens until the N22 units replace them.

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

### N14.5 Activity (built: N14.11)

- The heatmap is the minimal form of `decisions.md` (2026-11-19). On a deck page it follows the subdecks, not the counts and Study. Replaces N11.5's "On the deck page it follows the counts and Study". Built as N14.11.

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

### N14.11 Activity heatmap

The H-A minimal form of `decisions.md` (2026-11-19). Antoine's build direction (chat): it should look good and be simple to ship, and performance matters: it must not slow down the page. Data and rules are `stats/activity-heatmap`. Changes from the H-A page: a day cannot be tapped and has no callout; with no reviews the heatmap is hidden, with no empty grid or message; there are no month labels and no legend; the Mac adds a hover tooltip on each day.

- Grid: small rounded squares, one per day. Weeks are columns, the most recent week trailing; days are rows from the locale's first weekday. Days after today in the current week are not drawn. Standard SwiftUI shapes in a grid: no custom drawing, no gestures.
- Colour: a day with no reviews uses the system secondary fill. Days with reviews use four steps of the app's accent (tint) colour, from the system's hierarchical styles: level 4 primary, 3 secondary, 2 tertiary, 1 quaternary (levels: `stats/activity-heatmap` AH7). Today's square has a 1 pt outline in the primary label colour. System colours only, so dark mode and Increase Contrast need no extra code.
- Width: as many whole weeks as fit the row's content width, up to 53. The squares stretch so the strip spans the row from edge to edge; the gap is a fixed fraction of the square. Minimum column pitch: 20 pt on touch, 13 pt on Mac (about 16 weeks on an iPhone, 20 on the folded Duo, up to 53 on iPad, Mac and the Duo opened wide). No horizontal scrolling, date picker, month labels or legend.
- Text: one line under the grid in footnote secondary text, "312 reviews in the last 16 weeks", pluralised (AH8). It wraps and never truncates.
- No interaction: a tap does nothing. Accessibility: the grid and its line are one element, `home.activity.grid` (header `home.activity.header`), labelled "Review activity", with the summary line as its value. On Mac only, each square has a hover help tooltip with its date and count.
- Placement: a group of its own with the section header "Activity" and the side margins of the rows above it (never wider than the rows, never touching the edges). Deck page: after the Subdecks section, before the error line and the previous session. Today: after the decks with cards waiting. The tab layout's deck list (compact: iPhone, the Duo folded or held tall): after the deck rows. The deck list beside Today (Duo held wide) has none; Today has it. Mac: in the detail column, at the same places.
- Scope: the whole collection on Today and the deck list; the deck and its subdecks on a deck page (AH4).
- Hidden completely while the days shown have no reviews (AH8). It collapses only after the first result says so.
- Performance rules:
  - The page never waits for the heatmap. It loads asynchronously, off the main actor, on the query lane. Its height follows from the row width and is reserved from the first frame, so nothing moves when it appears. Returning to a page shows the cached grid at once while a due refresh runs in the background, without flicker.
  - One query per scope fetches all 53 weeks. A width change (rotation, window resize, fold) changes only how many weeks are drawn and never queries again.
- Not in this build: streaks, statistics, tapping a day, month labels, legend, scrolling back, colour choices, settings, the Home Screen widget.
- Replaces N11.5's Activity bullet for the heatmap only; its studied-today line and streak stats are unchanged by this section.

## N15. Round 2 Settings and sync decisions (2026-11-19)

Decided by Antoine in chat and page comments on 2026-11-19 (`decisions.md`: `settings.root`, `settings.accountSync`, `sync.connect`, `sync.status`, `sync.serviceSwitch`). Where earlier text differs, this section wins. The behaviour is `sync/policy` P25, `sync/cloudkit-recovery` V27 and `sync/meguri-provider` MP6; all text is `sync/status-strings` T19. Those spec sections were approved on 2026-11-19 (`docs/process/review-log/redesign-specs-20261119.md`). The points raised for the owner are in P25.6, V27.9 and MP6.8; two of them (no device backup of a replaced iCloud collection, and Cancel removing the Meguri Sync sign-in) were decided on 2026-11-19 and are marked there.

### N15.1 Settings page

- Settings is one flat page: a grouped form whose sections are Sync, then Collection. The sections' rows are on the page itself; no row pushes a form that only repeats a section.
- Mac, and the iPhone Duo's inner display held wide: the sections are a list beside the selected section, as in the system's Settings. Mac keeps N5's Settings scene and N11.4's search field; the list has Sync and Collection. The last selected section is restored (N5).
- Changed 2026-11-20 (first-run fix; approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs). In the Mac Settings window the selected section's title bar names it, so its form shows no second Sync or Collection heading. The flat page and the Duo's wide layout keep each section's heading.
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

- One thin grey bar along the very bottom edge, below the answer buttons, on every device: the system's linear progress indicator with the secondary colour as its tint (on the Mac a drawn bar, the exception below). No text, no counts, no accent colour.
- Touch: the bar spans the screen's width on its bottom edge, inside the bottom safe area, so the home indicator lies over its middle. iPhone Duo: the same edge folded, held wide and held tall; held wide it runs under both halves and across the fold. It is not a control, so it may cross (R9 covers controls).
- Mac: the bar is the window's last row, under the answer buttons, on the window's bottom edge and across its full width. The Mac had no bar before.
- Mac exception (2026-11-20; need: the bottom-edge, full-width bar above, which the system indicator did not meet; Antoine reported the gap below the bar and at its leading end on 2026-11-20 and asked for the bar to sit on the edge, PR #24): the bar is drawn, not the system's linear indicator, which draws its rounded track inset in its frame. The drawn bar is a 4 pt track in the quaternary fill with the value in the secondary fill; the window's rounded bottom corners clip its ends, as they clip any content on that edge. Its accessibility below is unchanged. Verification: `ReviewLayoutTests.progressBarIsTheBottomEdge` (light and dark), and the Mac review drives in the test VM on 2026-11-20 (light and dark, 800, 1000 and 1600 pt windows), in PR #24. Touch keeps the system indicator, which lies on the screen's bottom edge.
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
- The answer buttons' group is centred: on touch it is at most `ContentWidth.readable` wide and spans the full width when a fold crosses it; on Mac the four standard buttons are centred in the window, as is Show Answer.
- Replaces: N13.3's trailing-half Show Answer where no fold crosses the row.

### N16.4 Reading column

- Changed 2026-11-20 (Antoine asked "do we really want a max width at all?"; decided by the coordinator under his standing delegation; `decisions.md`): the card and any notice above it take the review's full width on every device, as on iPhone. Card styling is the user's and is written for a card that fills the window, so a narrower column would change how it looks. A card whose own content is wider than the window, such as a wide table or image, scrolls sideways inside the card page; any other card shows no horizontal scroll bar at any window width, also with scroll bars always shown.
- iPad and Mac keep the review's states (N16.6) in one centred column, `ContentWidth.readable` wide. A narrower window uses its full width. iPhone and iPhone Duo are unchanged: the card, its notice and the states take the full width in every position. The choice is by idiom, as N13.1's layout choice is. The touch answer row keeps its own width (N16.3).
- Around the answer row and the states the window shows the card page colour. A template's own background does not reach it yet (N13.4, R335).
- Replaced (2026-11-19 text): "iPad and Mac: the card, any notice above it and the review's states sit in one centred column, `ContentWidth.readable` wide".

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
- A notice above the card keeps its warning symbol and text, centred across the review's width (N16.4); a long one still scrolls within its limit.

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
- One HTML control for the focused field, the `chevron.left.forwardslash.chevron.right` symbol labelled "HTML Editor", selected while that field shows its HTML. Touch: at the leading end of the key row, with Previous Field, Next Field and Done; the bar is a system toolbar that replaces WebKit's own. Mac: at the leading end of the panel's bottom bar, opposite Add. ⇧⌘X stays. The control takes the identifier of the field it acts on (`editor.field.<ord>.htmlToggle`), and `editor.add.htmlToggle` while no field has had focus.
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

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, rows `home.goToDeck`, `home.deckActions`, `home.deckName`, `shell.commands`; round 6 options 1C, 2C, 3B, 5B). Where earlier text differs, this section wins. It covers the screens that exist. Delete Deck (4B), Deck Options (6B) and the review actions (7A, then round 7's 1D and 2A) arrive with their units, which add their own subsections; their places below are kept for them. The name rules of N20.3 are `decks/decks` D19, the create row's D18. Meguri authors these strings; implementers take them from N20.6.

### N20.1 Deck search in the sidebar

- Sidebar layouts (Mac, iPad at regular width): a search field at the top of the sidebar, in the system's sidebar placement for `.searchable`, with the prompt "Search or Create a Deck". It replaces the Go to Deck sheet.
- Typing replaces the sidebar's list, the Today row included, with the results, as System Settings does. Each result is two lines: the deck's last name component over its parents in secondary text, joined with " › " (a top-level deck has no second line). Matching and order are the touch Search's (N14.2): `decks/deck-list` H6.1 and H6.2. Filtered decks are listed.
- After the results, while `decks/decks` D18 finds no deck with the typed name: a row with the `plus` symbol, "Create deck named “<query>”". It opens N20.3's New Deck form with the query as the Name and Parent Deck None.
- With no result and no create row, H6.3's one-line message shows.
- Return or a click opens a result: its deck page, with the sidebar selection on that deck; a filtered deck shows R53's explanation (`decks/deck-list` H3). The query and results stay while it is open, and the opened deck shows selected among them. Esc clears the field and the tree returns, the opened deck still selected.
- Mac: after each edit the first result is highlighted, or the create row when nothing is listed; ↑ and ↓ move the highlight over all rows and wrap (H6.3). The highlight does not change the detail; Return opens the highlighted row. iPad: a tap opens a result; Return in the field opens the first.
- ⌘F (Edit › Find…, N20.4), ⇧⌘O (a second key of Find…, with no menu item of its own) and "/" in the `decks.list` and `decks.page` contexts (H9.2; never while a text field or other view owns the key, N4) put the focus in the field, showing the sidebar first when it is hidden. In the tab layout the same keys present the deck list's Search (N14.2). During review Find… is disabled.
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
- Confirming composes the full name and runs the create or rename of D19.5. A refusal shows in one line under the form, which stays with the draft. The existing-name rule is `decks/deck-list` H7's. A created deck, and a renamed one, opens its deck page in every layout.
- **Saved As** (H7's final-name report, D19.5): when the stored name differs from what D1 makes of the typed full name (a `+` was added, R61, or an existing deck's spelling adopted, R57), the form stays open after the write: its rows give way to one row, "Saved As", with the stored name as its value (its path joined with " › "), and a Done button replaces Cancel and the confirm button. Done closes the form and opens the deck page.
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
| Edit | Undo <step name>, Redo <step name> | ⌘Z, ⇧⌘Z | titled with the core's undo step name ("Undo Rename Deck", "Undo Answer Card"), the review's existing pattern, until `review/reviewer-strings` S5's catalog keys exist (R173); plain "Undo" and "Redo", disabled, when nothing can be undone or redone. During review they use the review session's own restore (session-aware); outside review, the collection's undo history. On Mac a focused text input keeps its own undo. On iPad text focus is not detected outside sheets, so a sheet keeps the system's text undo |
| Edit | Cut, Copy, Paste | ⌘X, ⌘C, ⌘V | the system's |
| Edit | Find… | ⌘F; also ⇧⌘O | the deck workspace shows in the main window, no sheet or panel owns the keys, and no review runs; focuses N20.1's field (tab layout: presents the Search). While it is disabled, a text view's own Find keeps ⌘F |
| Deck | Study | ⌘R | the shown deck's Study is available (N14.3); never for a filtered deck (H3) |
| Deck | Rename… | ⇧⌘R | a deck is shown, filtered included |
| Deck | New Subdeck… | | a normal deck is shown |
| Deck | Delete Deck | ⌘⌫ | a deck is shown, filtered included, outside review, and no text field, sheet or panel owns the keys (N4, H9.2); N20.7 |
| Deck | Deck Options… | ⌥⌘O | a normal deck is shown, outside review; N20.9 |
| Card | Show Answer | Space | review shows a question side |
| Card | Again, Hard, Good, Easy | 1, 2, 3, 4 | review shows an answer side |
| Card | Edit Note | ⌘E | a card is presented, on either side, and nothing else owns the keys; N20.8 |
| Card | Bury Card | ⌘J | as Edit Note; N20.8 |
| Card | Suspend Card | ⇧⌘J | as Edit Note; N20.8 |
| Card | Back to Decks | Esc | during review |

- Edit keeps the system's text items intact: its Find submenu and every other item the system adds for text. Meguri's Find… is a separate item placed just before that group; while enabled it takes ⌘F, while disabled the system's Find ▸ Find… of a focused text view keeps it. ⇧⌘O is a hidden second key equivalent of Meguri's Find…, with no menu item; on iPad it is a key command with no discoverability title.
- Separators: File after New Deck…, Export Collection… and Sync Now; Edit after Redo, Paste and Meguri's Find…; Deck after Study, New Subdeck… and Delete Deck; Card after Show Answer, Easy, Suspend Card and Back to Decks. A separator with nothing after it yet is left out.
- The Deck menu acts on the deck the workspace shows: the sidebar's selection (Mac, iPad), or the deck page shown (tab layout). With none (Today, the welcome, review) its items are disabled.
- Each command follows N4: disabled outside its context, routed through the action and guard of its visible control, and subject to input ownership. The Card menu's unmodified keys follow N11.3: the card page keeps the first claim. ⌘⌫ and the other menu key equivalents never take a key from a text input (H9.2). The one exception is Find…: while enabled it takes ⌘F even when a text input has focus, as its row above says.
- iPad with a keyboard: the same menus, items and keys in the iPad menu bar and in the list the system shows while ⌘ is held (SwiftUI `commands`), except the Card menu's single keys (Show Answer, the four ratings, Back to Decks): they carry no key equivalents on iPad, because the card host keeps the single key registration (N11.3's first claim), so the ⌘-held list does not list the review keys. The Card menu's ⌘ items (Edit Note ⌘E, Bury Card ⌘J, Suspend Card ⇧⌘J, N20.8) are not single keys and keep their equivalents there. iPhone and the iPhone Duo have no menu bar: with a keyboard the same keys act, and where the system shows its ⌘-held list it follows these menus.
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
| Saved As | the form's final-name row (N20.3) | new |
| Done | the form's button after Saved As | |
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
| Edit Note | the Card menu, the card's context menu (Mac), the touch review's More menu, the form's title | N20.8 |

- No longer used: "Go to Deck…", "Create Deck", "Overview", "Deck Actions", "Deck name", "Rename Deck…", "Close Review", the Study menu's title and its "Undo Answer" and "Redo Answer" items (Edit's Undo still reads "Undo Answer Card" after an answer, from the core's step name), "Import…", "Export…".

### N20.7 Deleting a deck

Decided 2026-11-20 (`decisions.md`, `home.deckActions`, option 4B). The operation is `decks/decks` D8; its summary D8.5.

- Entries: the deck menu's Delete Deck (N20.2, third group, destructive role, `trash`; filtered decks too) and Deck › Delete Deck (⌘⌫, N20.4), both on that one deck. No other entry; no swipe action (R49).
- Delete Deck first reads D8.5's summary for the deck. With `cards` 0 the deck is deleted at once, with no question: an empty deck, and a filtered deck, whose cards return home (D9). With `cards` 1 or more Meguri asks first. After the user confirms, the summary is read again right before the delete; when it differs from the one the question showed and `cards` is 1 or more, Meguri asks again with the new counts instead of deleting. A delete that asked nothing reads it the same way, so a card added after a zero-card read is asked about. A new generation in between ends the delete with nothing written. Changed 2026-11-21 (confirmed losses; decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review, `docs/process/review-log/delete-deck-losses-20261121.md`): the delete passes the counts of that last read (the question's when it asked, the zero-card read's when it asked nothing) as its confirmed losses (`decks/decks` D8 step 6), so a card or deck added between that read and the commit is no longer deleted unseen: the core refuses with `LossesChanged` and the new counts, writing nothing. Meguri then treats those counts as a fresh read: with `cards` 1 or more it shows the question again, in the same place, with the new message line; with `cards` 0 it deletes at once with the new counts. That refusal shows no `Couldn’t Delete` alert.
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
| iPhone; iPhone Duo folded or held tall; iPad at compact width | the system's confirmation dialog, which iOS 27 shows as a popover from the deck's row (earlier systems: from the bottom) | Delete Deck (destructive); Cancel where the system shows one, otherwise tapping elsewhere cancels |
| Regular width on touch (iPad, the Duo held wide) | a popover beside the deck's row, in the list's half; from the menu bar, the system's placement | Delete Deck (destructive); tapping elsewhere cancels, so there is no Cancel |
| Mac | an alert on the window, with the app icon | Cancel and Delete; Delete has the destructive role and is never the default, so Return deletes nothing; Esc cancels |

- Cancelling changes nothing.
- Deleting is one undo step, "Delete Deck" (D8 step 2): Edit › Undo Delete Deck (R173) on Mac and on iPad's menu bar restores the deck, its subdecks and cards; on iPhone, shake to undo where the system offers it. No banner and no toast.
- When the deck shown in the detail, or the deck page on the stack, is one of the deleted decks, the workspace returns to its root: Today in the sidebar layouts and on the Duo held wide; the deck list in the tab layout. N14.1's rule for a deck that leaves the collection covers it. Otherwise the detail stays.
- The default deck (D8, R68): Delete Deck on it removes its cards and subdecks and keeps the deck, renamed "Default" (or "Default+" when another deck has that name, D8's unique-name rule) and moved to the top level. It asks as any deck does. Afterwards it is an empty deck: D12 hides it when another top-level deck exists, otherwise its row shows its name with no count (AE1). The workspace returns to its root as above, since the deck shown was emptied.
- A delete that takes the backup of D8 step 4 shows nothing extra. A delete that fails writes nothing and shows an alert: the title `Couldn’t Delete “<name>”`, the message "Meguri couldn’t make a backup first, so nothing was deleted." for `SafetyBackupFailed` and "Nothing was deleted." for any other failure, and OK.
- While the delete runs, the deck's menu items and the Deck menu are disabled.
- Strings, all new: `Delete “%@”?`, the six messages above (a String Catalog entry with plural variations for S and C), "Delete Deck", "Delete" (Mac alert), "Cancel", `Couldn’t Delete “%@”`, "Meguri couldn’t make a backup first, so nothing was deleted.", "Nothing was deleted.", "OK", and Edit's "Undo Delete Deck" and "Redo Delete Deck" (R173's pattern).
- Replaces: R66's toast "Deleted <deck> (N cards)" with Undo (no toast); `home.deckMenu`'s "deletes … end with an Undo toast"; R67 stays as D8.5 states it.

### N20.8 Review actions: Edit Note, Bury Card, Suspend Card

Decided 2026-11-20 (`decisions.md`, `review.editNote`, option 7A; then `review.question`, round 7 options 1D and 2A, which replace 7A's separate Edit Note button). The review side of Edit Note is `review/edit-current`; Bury Card and Suspend Card are `review/card-actions` A4 and A5 on the current card. The card browser is not part of these rounds.

- **Touch toolbar.** Upper left Back to Decks (N11.1, N20.5). Upper right, leading to trailing: Undo; Redo right after it, only while a step can be redone (N11.1); then one More button, the system `ellipsis` symbol with the accessibility label and Voice Control name "More"; then the sync status button only while sync needs the person (N16.2). There is no separate Edit Note button.
- **The More menu** is a pull-down menu in groups, each item with its system symbol: [Edit Note (`pencil`)] | [Bury Card (`moon`), Suspend Card (`pause.circle`)]. Later card actions slot into these groups or new ones after them (`review/card-actions#A13`'s order: Card Info beside Edit Note; Flag and Mark Note; Set Due Date… and Forget…; Delete Note last and red); none is added now. Items are enabled while a card is presented, on either side, and disabled in the waiting, paused, finished and loading states and while an answer, an undo or another action runs (`review/commands#K1`).
- **iPhone Duo.** Back to Decks, Undo and More stack on the trailing edge, in that order from the top; the menu opens beside them. Opened flat, they and the menu stay on the trailing half, away from the hinge.
- **Mac: no toolbar control.** The Mac review keeps Back to Decks alone (N11.1). The Card menu gains, after Easy and before Back to Decks, in its own group: Edit Note ⌘E, Bury Card ⌘J, Suspend Card ⇧⌘J. A secondary click on the card shows Edit Note | Bury Card, Suspend Card first, above the system's text items (Look Up, Copy, Translate) when text is selected. iPad with a keyboard: the same Card menu items with their ⌘ keys in the menu bar and the ⌘-held list (N20.4: only the Card menu's single keys are left out of it). The `E`, `-` and `@` keys of `review/commands` keep acting on every platform.
- **Edit Note** opens round 5's Add Note form (N17.1) on the current card's note, titled "Edit Note", with Cancel and Done. Its Deck and Type group is replaced by the navigation bar's subtitle "<deck> · <note type>": the card's deck by its last name component, and the note type's name; neither can be changed here. The fields, the HTML control and the tags are N17.1's; the first field has focus. Placement, as Add Note's: iPhone, and the Duo folded or held tall, a sheet; the Duo held wide (any iPhone at regular width), the whole screen with the form in a centred column of the readable width; iPad, the system page sheet; Mac, a panel over the card that owns the keys (N11.1). A fold or unfold moves the same draft to the other presentation. Changes are a draft: Done saves them with one note update (`review/edit-current#E2`) and returns to the same card, rendered again, at the same place in the session (E3). Done with no change closes. Cancel, Esc, ⌘W and the swipe down close without saving; with a change they first ask "Discard changes?" with Discard (destructive) and Keep Editing. ⌘Return is Done on Mac.
- **Bury Card** hides the current card until tomorrow; **Suspend Card** hides it until it is unsuspended. Both act on the card, never on its note (`sched/manual-ops` M2, M3, one op naming the review session). The next card appears as after an answer (`review/card-actions#A4`), or the congratulations content when nothing is left. No banner and no toast: VoiceOver announces "Card Buried" or "Card Suspended".
- **Undo** covers the last change made on the review screen: an answer, Bury Card, Suspend Card or a saved Edit Note. Undoing a bury or a suspend puts that card back on screen on its question side. Redo keeps its rule (N11.1, iPhone Redo after Undo). The Undo control's label, Mac tooltip and Edit › Undo name the step in Apple's style (`review/reviewer-strings#S5`'s display names): "Undo Answer", "Undo Bury Card", "Undo Suspend Card", "Undo Edit Note"; VoiceOver hears "Answer undone", "Bury Card undone" and so on.
- **No unsuspend inside Meguri** (option 2A). There is no Suspended Cards list, no deck-menu item and no other way to remove a suspension in this version. Until the notes and cards browser arrives, a suspended card comes back only by Undo right away or by another client after a sync.
- Strings: "More" (new), "Edit Note", "Bury Card", "Suspend Card" (exist, `review/reviewer-strings#S18`), "Card Buried" and "Card Suspended" (new, VoiceOver announcements), the step display names "Answer", "Bury Card", "Suspend Card", "Edit Note" (new, S5), "<deck> · <note type>" (new), "Done", "Cancel", "Discard changes?", "Discard", "Keep Editing" (exist).
- Replaces: round 6's touch Edit Note button (7A's `pencil` beside Undo); `review/card-actions` A4 and A5's toasts and `review/reviewer-strings` S4's `review.toast.cardsBuried` and `review.toast.cardSuspended` for Bury Card and Suspend Card (no toast); S5's raw step names in the Undo titles ("Undo Answer Card", "Undo Bury"); R394's iPad "trailing inspector" and its panel as U190's editor in edit mode; R397's Close button (now Cancel and Done); R824 and R863's Esc that saves, for this form; `screen-inventory.yaml`'s `review.menu` toasts.

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

## N21. Round 8: Browse (2026-11-20)

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, rows `browser` and `home.deckList`; round 8 answers 1A 2A 3A 4B 5D 6A 7A 8A 9A 10A 11A, with the forgetting curve in the first version, system symbols on the Today and Browse rows, every bulk action in the first version, a Note menu and ⇧⌘B). Where earlier text differs, this section wins. Behaviour is `browser/workspace` and `search/filter-tokens`; this section is the layout. Approved with those specs by Antoine on 2026-11-20. Strings: `browser/workspace` V16 and `search/filter-tokens` T15.

### N21.1 Where Browse lives (11A)

- Mac and iPad: the sidebar always starts with Today (`calendar`) and Browse (`magnifyingglass`), each with its system symbol; deck rows keep text only. Selecting Browse turns the rest of the sidebar into the filters (`browser/workspace` V6); selecting Today shows the decks again. One window, three panes: sidebar, table, inspector. No separate Browse window.
- iPhone and the Duo folded or held tall: a third tab, Browse, between Decks and Settings; on the Duo, the middle symbol of the side tab bar (`browser/workspace` V14.3).
- Mac: View › Browse ⇧⌘B. The Browse row returns to Browse's kept state (V1).
- Round 10 (`decisions.md` `home.today`, 2026-11-20) makes Today a summary and gives deck rows waiting counts and Return, double-click and S to start a deck. Those apply with Today selected. With Browse selected the deck rows are filters: a click or Return sets the Deck token, S and double-click do nothing, and the number is Browse's count of the cards the value finds (T12.4).

### N21.2 The search field (2A)

- In the window toolbar, after the title and the Cards / Notes control (N21.3), as wide as the toolbar allows; on touch at the top of the list.
- Tokens are system search tokens (`searchable` with tokens) where the system's token view supports a token menu and the keys of T10; if it does not, the token view is a custom view, recorded as this screen's exception (need: T10's per-token menu and ⌘-click; verified against the system field's look in the VM screenshots). Each token shows the kind label in the secondary style then the values; an **is not** token adds "is not" after the kind; an advanced token is grey and shows its text. A flag value shows its colour dot.
- Suggestions are the system's search suggestions list, in sections (T12), each row with its symbol, label, secondary detail and trailing count.
- No syntax mode, no syntax reference, no error line: the field never shows Anki syntax errors (T5). The prompt is "Search".
- Esc closes the suggestions, then empties the query. The field's clear button empties it.

### N21.3 Mac layout (1A, 3A, 4B)

- Toolbar, leading to trailing: the title "Browse", the Cards / Notes segmented control, the search field, the Add menu (round 6's), the inspector toggle (`sidebar.trailing`). Nothing else: no action buttons (7A).
- Table: a native `Table` with sortable, resizable, reorderable columns; the state column first (V2.5); text leading, numbers and dates trailing, tabular figures; system selection; no row tints, no alternating colours beyond the system's.
- Status line under the table in the footnote style: V11's text at the leading edge, the column set menu at the trailing edge.
- Under about 900 points the inspector hides itself first; Return or a double click on a row shows it over the table.

### N21.4 The inspector (6A)

- Trailing pane with a segmented control Note | Info at its top.
- Note: the card preview (folds with its header), Deck and Type rows as round 5's form, the filled fields as labelled boxes, one "<n> Empty Fields" disclosure row, Tags as a token field, and a formatting bar under the fields (Bold, Italic, Underline, Cloze when it applies, Attach, HTML).
- Info: the stats rows, Review History, then the Forgetting Curve chart (Swift Charts, system style), as `cardInfo.card` draws them.

### N21.5 Several rows selected (7A)

- The inspector shows a title ("12 Cards Selected"), one secondary line (V7), then the commands as plain buttons in one column, each with its key at the trailing edge, Delete last in red. No toolbar change.
- The row context menu holds the same commands with their keys, grouped: [Change Deck…, Add Tags…, Remove Tags…] | [Flag ▸, Suspend, Unsuspend, Bury, Unbury] | [Set Due Date…, Forget…] | [Change Note Type…, Edit Note Type…] | [Find and Replace…, Find Duplicates…] | [Delete].

### N21.6 Sheets

- Change Deck…: a sheet with round 6's deck search and create row. Add Tags… and Remove Tags…: a sheet with a token field. Set Due Date…: a sheet with one text field (days, or a range "3-7"), its footer explaining the forms. Forget…: a sheet with the two options and "Review history is kept.".
- Find and Replace… (8A): a window sheet (Mac), form sheet (iPad, the Duo opened), full sheet (iPhone): the rows of V9 as a system `Form`, then the preview list, then a bottom line with "Undo restores every note.", Cancel and "Replace in <c> Notes" (the default action).
- Find Duplicates…: a small sheet with Field and Notes and Find.
- The Delete question: N20.7's forms (Mac alert; touch confirmation dialog).

### N21.7 iPad and the Duo opened (9A)

- iPad in landscape shows the Mac's three panes with touch-sized rows and fewer default columns; in portrait the sidebar appears from its toolbar button. Select (toolbar) starts Select mode, whose commands sit in a toolbar under the list: Change Deck, Tags, Suspend, Flag, More.
- The Duo opened: the list in the left half, the inspector in the right half; the Filters button opens the Filters sheet in the left half; nothing interactive on the fold; Select mode's bar stays in the left half.

### N21.8 Mac menus (7A)

Round 6's menu bar (N20.4) gains a **Note** menu between Deck and Card, and these items. Each command acts on Browse's selection while Browse is shown; outside Browse the Browse-only items are disabled.

| Menu | Item | Key |
|---|---|---|
| Edit | Select All | ⌘A (table focused); ⌥⌘A anywhere in Browse |
| Edit | Select Sibling Cards | |
| Edit | Find… | ⌘F: in Browse it focuses Browse's search field (round 6's deck search otherwise) |
| Edit | Find and Replace… | ⌥⌘F |
| Edit | Find Duplicates… | |
| View | Browse | ⇧⌘B (disabled while review presents a card, when Card › Show in Browse takes the key) |
| View | Note Types | ⇧⌘T (N22.1) |
| View | Cards and Notes (toggles) | ⌥⌘T |
| View | Card Info | ⌥⌘I |
| Format | Bold, Italic, Underline, Highlight, Text Colour, Superscript, Subscript, Lists ▸, Remove Formatting | `editor/formatting`'s keys, while a field has focus |
| Note | Edit Note | ⌘E (Browse: focuses the inspector's first field; review: round 7's) |
| Note | Add Tags…, Remove Tags… | ⇧⌘A, ⌥⇧⌘A |
| Note | Change Note Type…, Edit Note Type… | ⇧⌘M, none |
| Note | Delete Note / Delete <n> Notes… | ⌘⌫ (table focused) |
| Card | Show in Browse | ⇧⌘B, while review presents a card |
| Card | Change Deck…, Flag ▸ | ⌘D; flags ⌘1 to ⌘7, No Flag ⌘0 |
| Card | Bury Card(s), Unbury, Suspend Card(s), Unsuspend | ⌘J, none, ⇧⌘J, none (round 7's keys) |
| Card | Set Due Date…, Forget… | ⇧⌘D, ⌥⌘N |

- The Deck menu's items are disabled while Browse or Note Types is shown: Browse's deck rows are filters, not the shown deck. So ⌘⌫ is Delete Note in Browse and the focused list's Delete in Note Types, never Delete Deck.
- Card Info stays ⌥⌘I (R1226), not ⌘I, which is Italic while a field has focus.
- Separators: Note after Edit Note, Remove Tags… and Edit Note Type…; Card after Show in Browse, Flag, Unsuspend and Forget….

### N21.9 Other ways in (11A)

- The deck menu (N20.2) gains Browse Cards (`magnifyingglass`) after Add Note in group 1, for normal and filtered decks.
- The deck page gains, while the deck has suspended cards, a row "Suspended Cards" with the count, after Study's row (touch: in the summary group; Mac: under Study), opening Browse on Deck + Suspended.
- The touch review's More menu (N20.8) gains Show in Browse (`magnifyingglass`) after Edit Note in its first group; the Mac's Card menu has it with ⇧⌘B.
- A click on a heatmap day (Mac), or its menu (touch), opens Browse on that day's answers (V15).

### N21.10 iPhone and the Duo folded (10A)

- The Browse tab's navigation bar: the inline title Browse, then the Filters menu (`line.3.horizontal.decrease`: Filters…, Select, Note Types) and the Add menu as one glass group. The search field under it, with tokens.
- The list is an inset grouped list of two-line rows (V14.3), the count in its footer. Swipes and the context menu as V14.3.
- Filters is a sheet (Clear, Filters, Done) with Cards / Notes at its top, Sort By, Saved Searches and Filter By rows.
- A tap pushes round 7's Edit Note form; its More menu holds Preview, Card Info, the card's commands and Delete Note.

### N21.11 What this replaces

R1076, R1086, R1090, R1093 (canonical text, syntax mode and reference: dropped; pasted Anki syntax is still read), R1131 (middle-cut deck names), R1168 (a sidebar click replaces the whole search: it now replaces only its kind), R1193 and R1228 (Suspend toggle and its keys: two commands with round 7's keys), R1212 (Delete never asks: it asks when more than one note goes or the one note has more than one card), R1234 (iPhone chips: suggestions and a Filters sheet), and the earlier Browse text of `screen-inventory.yaml` `browse.browser`. Each requirement carries its note in the plan.

## N22. Round 9: Note Types (2026-11-20)

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, the two `noteTypes` rows; round 9 answers 1B 2B 3C 4B 5A 6B 7B 8B, with a preview that has a Light and Dark switch and no device picker, and ⇧⌘T). Behaviour is `notetypes/workspace` and `notetypes/draft-save`; this section is the layout. Approved with those specs by Antoine on 2026-11-20.

### N22.1 Where Note Types lives (1B)

- Mac and iPad: a Note Types row in the sidebar under Browse, with its system symbol (`rectangle.stack`, matching Today and Browse; decided by Claude under Antoine's delegation, 2026-11-20). Its detail is list · detail: a column listing the note types (filter field, + button) and the selected note type's detail with Fields | Cards tabs in the toolbar, Revert and Save (Save… when it will ask) at the trailing edge, and the title "<name>" with "<n> notes · <c> cards" as its subtitle.
- View › Note Types ⇧⌘T; File › New Note Type… (no key). ⇧⌘N stays New Deck…. R282's and R915's ⇧⌘N for Note Types are superseded.
- iPhone and the Duo folded: Settings › Collection › Note Types and the Filters menu of Browse push the list; no tab.
- Under about 1,100 points the sidebar hides itself while the Cards tab shows, as Mail and Xcode do with their navigators.

### N22.2 Fields tab (2B)

A native `Table` (Name, Sort, Used On, Notes with a share indicator) with drag reordering, + and − under it, and an inspector beside it holding Name, Collapsed in Editor, Right-to-Left and Placeholder (`notetypes/workspace` W4). No Advanced group in this version. A deleted field is crossed out until Save.

### N22.3 Cards tab (4B, 5A)

Three columns: the card types list (with counts and an issue dot) over the Fields list; the code editor with Front | Back | Styling and the Insert menu in its bar and the issues list under it; the live preview with its note menu, Question | Answer, Light | Dark (sun and moon), and the Cloze Number menu for cloze types. No device or size picker.

### N22.4 Styling (6B)

The Styling part shows "Shared by All Card Types" in its bar, and above the CSS one Card Text row: Font, Size, Alignment (a three-segment control), Text Colour and Background (colour controls). The row is absent when the CSS has no single plain `.card` rule (`notetypes/workspace` W10).

### N22.5 Save (3C)

One Save in the toolbar. When D3 asks, a sheet titled `Save Changes to “<name>”?` lists the changes (losses first, in red, each with its count), then the sync paragraph, then Cancel and the confirm button (Save and Upload with Meguri Sync; Save otherwise; destructive when content is lost). Progress while it saves shows in the toolbar ("Updating 212 of 412 notes").

### N22.6 New and Duplicate (7B)

New Note Type… is a sheet listing the six stock kinds as rows, each with its one-line explanation in secondary text, and a Name field; Cancel and Create. Duplicate is a direct command (⌘D, context menu).

### N22.7 iPad, the Duo, the iPhone (8B)

- iPad: the Mac's design with touch sizes; field rows open their options in a popover; Edit gives drag handles and delete buttons. Opening a card type hides the two lists: code and preview half each, the card type as a menu in the bar. A key row over the software keyboard: `{{` `}}` `<` `>` `/` `"` `=`, Insert Field, Done (Insert Field only with a hardware keyboard).
- The Duo opened: the list in one half and the note type's page (Card Types, then Fields sections) in the other; a card type gives the code one half and the preview the other, with one keyboard across the bottom; the side tab bar's tool is Light | Dark.
- iPhone and the Duo folded: pushed pages; fields editable; templates read-only with the preview (W13).

### N22.8 Change Note Type… (from Browse)

A sheet: From (the note type and the number of selected notes), To (a menu); two sections, Fields (one row per new field with a menu of old fields or Nothing) and Card Types (one row per new card type with a menu of old ones or Nothing; hidden when either side is cloze, R946); then What Will Be Lost (fields whose content is discarded and cards removed, with counts, or "Nothing." and the cards created); then, with Meguri Sync, `notetypes/draft-save` D4's sync paragraph. Cancel and Save (Save and Upload with Meguri Sync). Mappings and losses are `notetypes/change-notetype` M2 to M6.

### N22.9 What this replaces

N11.10's sheets for the template and field editors and its "No schema-change confirmation" bullet (replaced by N22.5's one question; its list of other questions is replaced by `notetypes/draft-save` D8), `screen-inventory.yaml`'s earlier `noteTypes.*` layouts, R919's per-row Fields and Cards actions, R920's "1: Name" list and Reposition, R925's full option list (W4), R931's card type picker, R932's Options menu, R938's sheet-sized editor, R940's platform classes, and R951's "Ordinary edits show no full-upload warning" for Meguri Sync.

## N23. Round 10: the sync status panel (2026-11-21)

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, row `sync.statusPopover`; round 10 answer 6A). Changed 2026-11-21 (round 10). In force: approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/round10-sync-panel-20261121.md`). The text is `sync/status-strings` T20; the status itself is T19.4 and T19.5, unchanged. Where the earlier text that N23.5 lists differs, this section wins. The sidebar and its frame are not part of this section.

### N23.1 The panel

- The sync status button opens one small panel: a popover on Mac and on iPad at regular width, `ContentWidth.primaryAction` (320 pt) wide and as tall as its content; at compact width (iPhone, the Duo folded or held tall) a sheet with the same content, the medium and large detents and the drag indicator, which scrolls at the largest text sizes. Both services, every state.
- Top to bottom, with no inner box, form group, divider or section header:
  1. One status line: the state's symbol (N23.2) leading, the status title beside it.
  2. Under the title, in secondary text, the status detail: one sentence, or none when the status has none. A detail that repeats the title is not shown, here or in the Sync section.
  3. While the work runs (below), one linear progress bar under the text, and under it, while media transfers and a count is known, "N media files left" in footnote secondary text (T20).
  4. One row of buttons at the trailing edge: Sync Settings, then the state's action (N23.3). When the two do not fit on one line (the largest text sizes), they stack, trailing, the action first.
- The panel shows what the Sync section's status row shows: its symbol, title, detail and action come from the same main status (T19.4, T19.5), so the two never disagree. It leaves out the rest of that row: T4's lines, T5's other media lines, T7's step and result lines, T19.3's setup-done lines, an iCloud connection failure's line (`settings.sync.connectionError`) and Meguri Sync's media facts (T18.6) stay in the Sync section, which now shows those media facts under its status row. With issues, one footnote secondary line under the detail gives their count, as the Sync section's issues row reads (`sync.issues.row`, or Meguri Sync's `sync.meguri.media.notTransferred`), as plain text; the list opens from the Sync section (N15.4). This line goes beyond 6A's "one sentence" on purpose: without it the panel would say less than the Sync section about the same status.
- The bar: for Meguri Sync it is the Account row's indicator, with the same value, whenever that row shows one (`sync/meguri-provider#MP6.5`, "One progress indicator": during the media step files done over done plus left, indeterminate until the first count); otherwise it is indeterminate while Meguri Sync signs in or a round exchanges. For iCloud it is indeterminate while the state shown (T19.4 rule 2: `stateWithoutAttention` under `needsAttention`) is `syncingCollection` or `syncingMedia`. While Meguri Sync prepares a sign-in, the Account row shows its labelled spinner (`sync.meguri.signIn.preparing`); the status is still the sign-in row, so the panel shows that row and no bar until signing in starts. A status that needs you, and a blocked or stopped status, has no bar (MP6.5). iCloud's `syncingMedia` has no detail (T3); its files line is under the bar.
- Only the toolbar button's symbol turns, exactly while the bar shows (N23.2). The panel's and the Sync section's symbols stand still.
- The panel follows the status while it is open: a change of state changes its text, bar and buttons in place. The person closes it (Esc, a click outside, the sheet's drag); it also closes when one of its buttons opens the Sync section, when a window sheet opens (for example `sync.keepCollection` by itself), and when the size class changes.
- Mac: Return runs the default button, when there is one; Esc only closes the panel. With keyboard navigation on, the buttons are in the panel's Tab order.
- Accessibility: the panel is `sync.status.popover`; the status line, its detail and the issues line are one element, `sync.status.panel.status`, read as the title, the detail, then the issues line; the bar is `sync.status.panel.progress`, labelled with the title, with the system's value; the files line is `sync.status.panel.filesLeft`; the buttons are `sync.status.settings`, `sync.status.syncNow` and `sync.status.action`. Choose… keeps its accessibility label `sync.decision.action.label`, as in the Sync section.

### N23.2 One cloud family

The toolbar button, the panel's status line and the Sync section's status row use the same symbol, one of five shapes, for both services. Shape first: only needs you has a colour, the system orange on the whole symbol. This extends the decision's "in the toolbar" to the panel and the Sync section, so the three never disagree.

Apple reserves the `icloud` symbols (`icloud`, `checkmark.icloud`, `arrow.trianglehead.2.clockwise.rotate.90.icloud`, `exclamationmark.icloud`, `icloud.slash`) for Apple's iCloud service: the SF Symbols metadata says they "may only be used to refer to Apple's iCloud service" and "may not be modified". So iCloud uses them, and Meguri Sync and Sync Off use the same five shapes drawn from the unrestricted `cloud` symbol with an unrestricted symbol inside it (N23.4, the exception).

| Shape | Means | iCloud | Meguri Sync and Off | iCloud states (`sync/policy#P14.2`, T19.4) | Meguri Sync states (T19.5) |
|---|---|---|---|---|---|
| cloud with a check | up to date | `checkmark.icloud` | `cloud` with `checkmark` | `upToDate` | round idle, Sync Automatically on |
| cloud with two arrows | something moves | `arrow.trianglehead.2.clockwise.rotate.90.icloud` | `cloud` with `arrow.trianglehead.2.clockwise.rotate.90` | the state shown (T19.4 rule 2) is `syncingCollection` or `syncingMedia` | signing in, preparing, transferring the collection or media, round exchanging |
| cloud | waiting, or not sent yet | `icloud` | `cloud` | `changesPending`, `offline`, `paused{automaticOff}`, `iCloudUnavailable{temporary}`, a waiting row of T19.4, no status yet | setup required before preparation, the keep-collection sheet open, round pending, waiting or cancelled, Sync Automatically off, a waiting row, media blocked, any other title |
| alert cloud | needs you | `exclamationmark.icloud` | `cloud` with `exclamationmark` | every needs you row (T19.4) | every needs you row (T19.5) |
| cloud with a slash | sync off | — | `cloud` with a slash | no service selected: the Service control reads Off, and an iCloud projection then reports `localOnly` (with iCloud selected, `localOnly` is T19.4 row 16, needs you) | |

- The two-arrows shape turns with the system rotate symbol effect, the arrows only, exactly while the panel's bar shows; with Reduce Motion on it does not turn.
- An issues row keeps the symbol of the state underneath (N15.4).
- N16.2's rule for showing the button during review is unchanged and separate: Meguri Sync signed in with setup not continued shows the button with the plain cloud, since T19.5 gives those rows no kind, or waiting. Only needs you rows use the alert cloud.
- The button's accessibility label is the service's name (`sync.button.label`, `sync.meguri.button.label`), and with no service `sync.button.label.off` (T20.2); its value is the status title without its "…"; on Mac its help tag is the title and the detail. The state is never given by the symbol alone.

### N23.3 The action

The first row that applies decides, in this order.

| Status | Buttons, trailing |
|---|---|
| Sync is off (no service, and the provider does not need the person) | Sync Settings, as the default button |
| Needs you, with a built action (T19.4, T19.5 Action column) | Sync Settings, then the action, as the default button unless the "No default button" bullet below says otherwise |
| Needs you, with no built action (iCloud's `noAccount` and `restricted` states, Update Meguri (T19.4 row 4), Abandon Change… (row 25, not built)) | Sync Settings, with no default button: Return does nothing |
| Not needs you, with the row's own action (Cancel Setup while setup prepares; Try Again for an acceptance that is not known, T19.1) | Sync Settings, then that action; Cancel Setup is never the default |
| Meguri Sync with no round reported yet, or with round media blocked | Sync Settings, then Sync Now, disabled, as the default button, with no help tag; File › Sync Now and Y still open the Sync section (T18.7) |
| Any other status with a service | Sync Settings, then Sync Now as the default button |

- Only the status action counts (T19.4 rule 1, T19.5's Action column). The actions of an issues row (Retry, Check Database…) stay in its list.
- A built action shows also while it cannot run, disabled, with no reason line (Try Again is enabled as Sync Now is, T9.2). An action whose conditions do not apply (Cancel Setup while nothing prepares, Cancel Replacement… with no operation, Meguri Sync's Choose… while its work runs, Copy Debug Info with no diagnostics) is not shown. For a needs you row the panel is then the third row's (Sync Settings, no default button); otherwise Sync Now keeps its place.
- No default button: an action that runs a collection replacement at once (Finish Setup, Resume of a waiting replacement) and Cancel Setup. Return then does nothing. Actions that open a confirmation first (Cancel Replacement…, Set Up Again…, Choose…) can be the default: their confirmation is the step that changes anything, and its destructive button is never the default (N11.8).
- Sync Now stays in its place while the status moves, disabled, even where `sync/policy#P12.1` would let a press join the running exchange, so the buttons never jump. Outside the panel nothing changes: File › Sync Now, Y and the Sync section keep their availability and their reason (T19.7); the panel shows no reason line, and the Mac help tag of its Sync Now gives the reason where T19.7 has one. A running Sync Now has no Cancel in any status surface (the panel, the Sync section, T19.6).
- When the state needs you, its action takes Sync Now's place: a disabled Sync Now with a reason would only repeat the sentence.
- An action that runs at once runs from the panel, which stays open and updates (Sync Now, Try Again, Resume, Finish Setup, Copy Debug Info). An action with a confirmation (Cancel Replacement…, Change the service recovery's action): on iPhone and iPad its confirmation shows over the panel, as the Sync section's row does; on Mac the panel closes and the action opens the Sync section, whose row asks, since a transient popover can close when its alert takes the keys. Meguri Sync's sign-in action is Sign In… and opens the Sync section at the sign-in fields; an action that opens a sheet of the Sync section (Meguri Sync's Choose…, a setup started again) opens the Sync section, which presents it (N15.3). iCloud's Choose…, Set Up Again… and Resume Syncing open their sheets directly, as built (for Resume Syncing see N23.5). On iPhone the panel's sheet closes before the Sync section or another sheet opens, so no sheet sits on a sheet.
- Copy Debug Info is in the panel only where it is the status action: iCloud's `setupNeeded` state and Meguri Sync's blocked provider, the two states of the decision; Meguri Sync's collection it cannot use (`localUnusable`, added 2026-11-20); and iCloud's damaged-collection row (T19.4 row 1), where, until the damaged-collection screen is built, the built action is Copy Debug Info in place of Show Repair Options. Everywhere else Copy Debug Info is in Troubleshooting (N15.5 row 6). While it copies it is disabled, as in the Sync section.
- Sync Settings opens the Sync section: the Settings window on Mac, the Sync section of Settings on iPad and iPhone, and during a review on iPhone and iPad a sheet over the review (N15.4).

### N23.4 The exception: Meguri Sync's cloud shapes

- Need: compatibility with Apple's terms for SF Symbols (above). No unrestricted SF Symbol puts a check, two arrows, an alert or a slash inside a cloud.
- What is drawn: the view `CloudShapeSymbol` (`DesignSystem`, in the component gallery) shows the unrestricted `cloud` symbol with `checkmark`, `arrow.trianglehead.2.clockwise.rotate.90` or `exclamationmark` inside it, or a drawn slash across it, in the style of the `icloud` shapes. Apple's symbols are drawn as they are, and `icloud` symbols refer to iCloud only.
- The inner symbol has a fixed weight and about 0.42 of the cloud's size, which matches the `icloud` shapes at their usual size. It does not follow a weight set by its place; none of the three places sets one.
- Why a view and not a custom symbol: Apple's way is a custom symbol made in the SF Symbols app, which this project cannot run unattended, and the system draws symbols only as images, so no outline can be taken from them to build a template. The view keeps the font, weight, foreground and rotate effect of its place (the effect turns the inner arrows only); it does not get a custom symbol's rendering modes or variable values, which these places do not use. A custom symbol made from the same unrestricted symbols replaces it when one is made.
- Verification: the built app in the test VM, each shape beside its `icloud` counterpart, light and dark, regular and the largest text size, on Mac, iPad and iPhone (the pull request's captures).

### N23.5 What this replaces

- N11.8, first bullet: "A distinct SF Symbol per `sync/policy` P14.2 state" becomes N23.2's five shapes (colour is still never the only signal, R104), and its "sheet with medium and large detents" stays, with the drag indicator. The `sync.status` screen contract's popover description and symbol lists for both services are replaced by N23 (that contract has a new version for N23).
- N15.4: "in the Sync section's status row and in the status popover" stays for the needs you sentence and its button; in the panel the issues row is a plain count line, and its list opens from the Sync section.
- `sync/status-strings`: T2's popover row; T3's `sync.status.keptLocally` "in the popover only" (not shown since T19); T6's reason line under the popover's Sync Now and its "beside the disabled Sync Now" for decisionRequired; T7's steps and results "in the popover" (the end of a Sync Now the person started is still announced, T1); T8's throttling line "only in the popover" (it shows in the Sync section's status row); T9.2's reason line under Try Again in the popover; T18.3's service label in the shared popover (the panel names no service; the button's label does); T18.7's three popover buttons (`sync.meguri.setup.continue`, `sync.meguri.now.chooseCollection`, `sync.meguri.media.showAffected`), which stay File › Sync Now's routes; T19.4 row 5's "no sheet follows" for Resume Syncing: until V27.6's direct resume is built, Resume Syncing opens the connect flow's continue page, as built, in the panel as in the Sync section; T19.3's setup-done lines "and the popover"; T19.4 rule 3's issues row "which opens the list", in the popover; T19.7's popover Sync Now with its reason.
- Plan text, for the popover only: R103 (offline and pending changes now share the plain cloud; the title in the panel, the help tag and VoiceOver tell them apart), R105 and R1560 (collection and media progress, the last exchange, pending work and the latest error: the panel shows the status, its bar and its sentence; the rest is in the Sync section).
- The popover's inner form group, its divider, its centred Sync Now with a reason line and its centred Sync Settings.
## N24. Round 10: Today and the sidebar (2026-11-21)

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, row `home.today`; round 10 answers 1A, 2A, 3A, 4B, 5A and 7A). Changed 2026-11-21 (round 10). In force: approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/round10-today-20261121.md`). Behaviour that is not layout is in `decks/deck-list` H3 and H12, `stats/activity-heatmap` AH7 and AH9, and `stats/studied-today` T12. Where the earlier text that N24.7 lists differs, this section wins. N23 is round 10's sync status panel. The sidebar's frame, insets and row layout are unchanged: this section changes only what a row's number counts, its help tag and its keys.

### N24.1 Where the summary shows (5A)

- Sidebar layouts, where the decks are in a list beside Today: the Mac, the iPad at regular width, and the iPhone Duo unfolded and held wide (Today beside the deck list, N14.1, N14.10). Today is the summary of N24.2 and lists no decks. "The Duo opened" is read as held wide: held tall it is one stack (N13.1) and keeps the deck list; this reading is raised for Antoine in the pull request.
- Compact layouts (an ordinary iPhone, the Duo folded or held tall, an iPad at compact width) keep the deck list as the Decks root (N14.2), with the heatmap under it (N14.11) in N24.6's levels and colours. No summary and no Study for everything there (round 1: no Study bar).

### N24.2 Today, top to bottom (1A, 3A)

The title and totals stay as N14.4 and N14.1 say (Mac: the toolbar title "Today" and the totals as its subtitle; touch: the title and the totals line; both hidden while nothing waits). Then one list in the platform's style (the Mac's plain list; on touch the inset grouped list of the deck page, N14.3), with the Study group, the two rows and Activity as four sections, only Activity with a header:

1. **Study.** The deck page's Study button (its style and availability, N14.3, also its minimum target on touch), with two lines beside it on the Mac and under it on touch, in secondary text: the first "492 cards in 4 decks · about 35 min" (one deck: "40 cards in Languages · about 3 min"; the estimate only when T12 gives one), the second, with more than one deck, the order: "Languages first, then Science, Culture and Music Theory". Both wrap and never truncate. The cards and decks are the walk's (N24.3): the top-level normal (not filtered) decks with cards waiting, and their cards waiting (the deck list's counts, `sched/counts` C5, which Today already holds; no other read). The totals above count every top-level deck, filtered ones included (N14.1), so they can be larger; accepted. Study starts N24.3's walk; Return and S start it too (H12: the sidebar's Today row selected, S there on the iPad only; or on the Mac Today's page focused).
2. **Studied Today.** One row that opens nothing: the label "Studied Today", a linear progress bar (the system's linear indicator, the accent colour) whose value is the cards studied today over those cards plus the cards waiting in the walk's decks (item 1's number), then the number of cards and the time, the time written as the finished screen writes one (N16.5) in the system's short unit form; "None yet" with nothing studied today, and then an empty bar (T12).
3. **Previous Session.** One row that opens nothing, only after a session with at least one answer: the label "Previous Session", the deck's last name component with its parent in secondary text ("Vocabulary in Español"), then "46 cards · 8:12 AM" (the session's different cards, and when it ended: the short time today, "Yesterday" and the time for the calendar day before in the device's zone, else the date; a session that never ended shows its last answer's time). After a walk in this launch the row names the walk's decks with the list format ("Languages, Science and Culture"), and its cards and end time are the walk's (as for one session, the time is when it ended, not how long it took); after a relaunch it shows the last deck's session, which is what the session store keeps: a walk is not stored as one record (follow-up, recorded in `docs/process/verification-status.md`). A deck page shows a previous session only for a single-deck session of that deck.
4. **Activity.** The heatmap (N14.11, with N24.6), its header and line.

Then the collection's error line, as before (not beside the deck list, N14.1).

- **Nothing waiting** (the walk would have no deck): in place of item 1, "Nothing to study right now" and, when a learning card becomes ready later in the scheduler day, "Next learning card at 2:40 PM" (the earliest `next_learning_ready_at` of `sched/counts#C7` among the top-level normal decks, in the short time style; after midnight and before the rollover it can read "1:30 AM", which is accepted). Items 2 to 4 follow.
- Loading, no collection, failed and no-decks states keep their content (N14.1).
- At the accessibility text sizes (touch) each row stacks its value under its label.
- Accessibility: Study is `home.today.studyAll`, labelled "Study All Decks", with the input labels "Study" (first) and "Study All Decks", and the first line as its value; the lines are `home.today.studyAll.summary` and `home.today.studyAll.order`; the rows are `home.today.studied` (one element: "Studied Today", value "128 cards, 19 minutes") and `home.today.previous`; the empty line `home.today.empty`, the learning line `home.today.nextLearning`.

### N24.3 Study walks the decks (7A)

- **The list.** When Study is pressed, the walk's list is fixed: the top-level normal (not filtered) decks with cards waiting in the deck list's counts, in the deck list's order (`sched/counts` C1). Filtered decks are never in it (H3). Each turn is one ordinary session of `sched/review-session` for that deck: it studies the deck and its subdecks with the deck's own limits and order (`sched/queue-building` B2), its queue is built when its turn starts, and every answer is the answer that deck's session records. Nothing is written beyond each deck's ordinary start (H3's selection write), and no queue mixes decks.
- **Turns.** When a deck's session has nothing more to show now, the next deck starts at once in the same review screen: no screen, title or pause between them, and the review presentation stays up. A session that would show "Waiting for Learning" (N16.6), whose only cards left are learning cards not ready yet (learn-ahead included, C7), is left for the end instead. After the last deck the walk goes on round its list again, in order, with each deck that has a card ready now; it ends when no deck of the list has one, or when a whole pass answers no card, so it always ends. A deck deleted or emptied before its turn, or whose start finds no card, is skipped. A deck not in the list that gains cards is not added: Study shows again on Today afterwards.
- **The end.** One finished screen (N16.5): the number counts the different cards answered in the whole walk and the time is the walk's active study time. Its Keep Going is the open deck's, as for one session: it shows while that deck's learning cards are ready again, and after it the walk goes on as in Turns. No wake runs for the other decks: when another deck's learning cards become ready while the finished screen shows, Keep Going does not show for them; Done returns to Today, which offers Study again. The walk never shows Waiting for Learning.
- **Stopping.** Back to Decks (and Esc) stops the walk: the open session ends as any session ends, no other deck starts, and the window returns to Today. N11.6's summary sheet, when it shows, gives the walk's numbers. When the next deck cannot start (a refused start, F5.3; the collection held by sync; the window no longer active), the walk stops there: the window returns to Today, with F5.3's refusal when it applies and N11.6's summary sheet as after Back to Decks; what was answered stays answered. One case differs: when the next deck's session has already opened and pausing it cannot save its record (the window stopped being active as it started), the review stays on screen with its error, as after a session that cannot end, and Back to Decks closes it. Quitting during a walk recovers only the open session, as today; the walk does not resume.
- **Undo.** Undo and Redo reach the open deck's answers only: at the start of the next deck Undo is disabled until that deck's first answer.
- **Progress.** The session progress bar (N16.1) covers the whole walk: the walk's answers over its answers plus the open deck's cards left (its queue counts) plus the cards still to come: for a deck not started yet, its cards waiting as the deck list's counts gave them when the walk started; for a deck already studied and left for the end, the cards its session had left when it closed.
- Review's toolbar, keys, notices and states are those of one session; nothing names the deck being studied (the card's own template may). When the walk ends, the sidebar's rows, Today's counts, T12's numbers and the heatmap refresh, as after any session.

### N24.4 The sidebar's deck rows (2A)

- A deck row's number counts every card waiting: new + learning + to review (the touch rows' one number, N13.2, which the iPad already showed, and Today's totals). It is hidden at zero, as before. On the Mac it is the row's badge; on the iPad the trailing number, unchanged.
- Hovering a row shows its help tag: the deck's full path (" › " between the names) on the first line, the three counts on the second ("6 new · 23 learning · 382 to review", zeros included). The Mac, and the iPad with a pointer. The row's accessibility value uses the same three counts ("to review", in place of "due").
- A click (a tap) opens the deck page, unchanged. A double-click (the Mac only: on the iPad a pointer's double-click cannot be told from a finger's double tap) or, while the deck list has focus with the row selected, Return start the deck's review, and so does S on the iPad; as `decks/deck-list` H3 and H9.2 say (built now: H12). On the Mac sidebar S is not built: the system list's type-select keeps the letter keys, a platform default (changed 2026-11-21). Exception (round 10, Mac): Return is taken by a key monitor of the main window, not by the list. Need: a row's click leaves keyboard focus with the row's own button rather than the list, so neither the list nor its primary action (`contextMenu(forSelectionType:primaryAction:)`, tried first) ever sees Return. It acts only on Return or Enter without Command, Option, Control or Shift (Caps Lock does not count), not on a repeat, in the main window with no sheet, and while no text input, sheet of the app's own or review has the keys (`SidebarReturnKey`). Verification: `SidebarReturnKeyTests` for each guard; `MacDeckListKeysTests` in the test VM for Return on a deck row and on Today, the double-click, and S in the search field. The first click of a double-click opens the page, as a click does; the second starts the review. A deck with nothing to study shows what H3 says (its finished state, or H5.4's empty view); a filtered deck shows R53's explanation.

### N24.5 iPad and the Duo opened (5A)

- The same summary as the Mac, in touch sizes, at the deck page's inset (N14.3). The Duo held wide keeps the list beside Today with its Today row (N14.10), whose number is the cards waiting.

### N24.6 The heatmap's look (4B)

- **Levels**: `stats/activity-heatmap` AH7, rewritten 2026-11-21, gives each of the four tints to roughly one fourth of the person's studied days in view, with the top day always in the top tint. Because only the days in view count, a narrower or wider grid may shade a day differently; accepted.
- **Colours.** A day with no reviews keeps the system secondary fill. The four steps are the accent (tint) colour at the opacities 0.35, 0.55, 0.8 and 1 in light mode (0.4, 0.6, 0.8 and 1 with Increase Contrast, over the fill's own Increase Contrast value) and 0.5, 0.68, 0.84 and 1 in dark mode, with or without Increase Contrast, where a weaker first step sat too close to the empty-day fill, so they follow the person's accent colour on the Mac as the rest of the app does; with the Mac's Graphite accent the steps use the system blue, since grey steps would not tell a light day from an empty one. `ActivityGridTests` fixes the level-to-opacity table. Exception (round 10): the system's four hierarchical opacities of the tint sit too close together, which round 10's option page showed as "almost every day is the same dim blue" (option 4B's note: "explicit steps of the accent instead of the system's four opacities … a custom colour value, justified by legibility, to be checked in the built app in both appearances and with Increase Contrast"). Verification: the built app in the test VM in light and dark (an empty day beside level 1), with Increase Contrast, in the pull request; the Graphite accent is not reachable in the VM's routes and is checked in code only.
- **Month labels** (AH9): above the grid, in caption secondary text, inside the grid's one accessibility element (not read on their own). The grid's height includes the labels' row from the first frame (N14.11's performance rules).
- **Today** keeps its 1 pt outline in the primary label colour.
- The Mac's day help tag and the summary line are unchanged. Today's square counts AH2's rows, Studied Today counts T12's (N6's): on a day with Set Due Date or rescheduling rows they can differ; accepted.

### N24.7 What this replaces

- N14.1: its third bullet (the decks with cards waiting, each with Study), "Only the previous session keeps a section label", the nothing-waiting order ("The error line and the previous session follow") and "iPad's Today stays a plain list", for the sidebar layouts.
- N14.6: "The count is the To Review badge, hidden at zero (N11.5)", and N11.5's native badge with the To Review count.
- N14.11: the Colour bullet, "there are no month labels" (opening paragraph, Text, Width and Not in this build) and its Today placement ("after the decks with cards waiting").
- N16.5: "counts different cards answered in the session" becomes "in the walk" for a walk.
- `stats/activity-heatmap` AH7's earlier rule.
- Screen contracts: `home.deckList` has a new version (its badge and Today text); `home.today` is added to `screen-inventory.yaml` with the code.
