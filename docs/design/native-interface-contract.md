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
- On macOS, keep the icon-only Review actions menu visible at the upper right (N20.8).
- Keep a quiet transfer-state icon at the upper right. Its popover explains the real recorded state. During review it shows only while sync needs the person (N16.2).
- Do not infer remote freshness from a prior transfer, saved credentials or the absence of errors.
- Keep Show Answer and the four rating actions at the bottom of the review area.
- Reserve the action region on both sides so reveal does not move or resize card content.
- Long card content scrolls between the corner controls and the answer region, and on under the answer region (N16.9). On touch it also scrolls on under the top bars, to the screen's top edge (N16.12).
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
- On question and answer, unmodified R replays through the existing audio action. Typed-answer and editor focus, navigation, modifiers and repeat suppression keep their existing ownership (authorized 2026-11-25).
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

Integrations, Antoine's explicit design of 2026-11-25: two native checkbox rows,
each with a small trailing round light and plain short receiver state. Keep the
port in the checkbox label; normal status says Listening or Off without repeating
the address. The narrow indicator exception uses the system `circle.fill` symbol
in caption-2 size: semantic green for a bound receiver, secondary for Off or a
wait/start, orange for Port in use. Text always identifies the state; the symbol
is decorative and hidden from accessibility. Listening means the socket bound,
not that a client connected or the checkbox is enabled. Keep Waiting and Starting
distinct, full state/remedy details in native hover help and accessibility hints, and
actionable collection/destination errors inline. No separate redundant status
rows or Destination footer. Native verification remains required at the public
Settings width, including keyboard/VoiceOver, light/dark and conflict states.

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
- Upper right, leading to trailing: on iPhone and iPad the undo button (U179) and the More button (U182; the `ellipsis` symbol labelled "More", whose menu N20.8 gives; no toasts); then the sync status icon, only while sync needs the person (N16.2; N3; `sync.status`'s button, whose states that contract gives, N11.8). The Mac has the icon-only Review actions menu (N20.8, approved 2026-11-24); undo and redo stay in the Edit menu, card actions also stay in the Card menu and the card's context menu (R288).
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
- iPhone Duo held wide unfolded: the detail beside the deck list shows Today until a deck is picked. The list beside it leaves out its Previous Session section whether Today or a deck is selected, and Today leaves out the collection's error line, which the list shows: nothing appears in both columns.
- Today: the collection's totals ("20 new · 3 learning · 72 to review", the top-level decks' counts), then, with no section header or rule, the decks with cards waiting, each with a Study button that starts that deck through the deck page's Study path. A deck is listed when it has cards waiting and no subdeck with cards waiting; a parent is also listed, before its subdecks, when it has cards of its own beyond theirs. A nested deck shows its parents as secondary text under its name ("Languages › 日本語"); the list has no indentation. Mac rows show the non-zero counts by name; touch rows show one number (N13.2), which VoiceOver reads as "N cards waiting". At the accessibility text sizes a row stacks the count and Study under the name. Study keeps the deck page's minimum target.
- With nothing waiting, Today shows only "Nothing to study right now": no totals and, on Mac, no subtitle (N14.4). Only the previous session keeps a section label. The error line and the previous session follow.
- Compact layouts (iPhone, the Duo folded or held tall) keep the deck list as the Decks root and have no Today.
- Loading, no collection, failed and no-decks states keep their content; the page is still titled "Today" where the sidebar's first row says so.
- A selected deck that is no longer in the collection (deleted, or removed by a sync) loses the selection: the sidebar layouts return to Today. A hidden deck keeps its page.
- iPad's Today stays a plain list while its deck page is grouped (N14.3); Antoine accepted both on the iPad capture.
- Replaces: N11.5's Decks root as the sidebar layouts' detail, N12.1's "All Decks" first row, N13.1's "Select a Deck".

### N14.2 Tab-layout deck list

- The toolbar has the sync status button, a Search button (`magnifyingglass`, `home.collection.search`) and one Add menu (`plus`): New Note and New Deck…. There is no Go to Deck or Create Deck button and no Study bar.
- Search presents the system search field, with the system's own "Search" prompt (changed 2026-11-22; it read "Search or Create a Deck", N20.1), with the keyboard; until then the field stays out of the way. The title is N14.8's. Typing replaces the tree with the matching decks, using Go to Deck's filter: each shows its last name component with its parents as secondary text underneath, as Today does; a row opens that deck. When no deck has the typed name, a "Create deck named …" row opens the Create Deck sheet with the name filled in.
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
- Colour: replaced by N24.6's Colours (changed 2026-11-21: the system green, not the accent). A day with no reviews uses the system secondary fill. Changed 2026-11-24 (owner follow-up): on Mac Home, today's square has the same count-based style as every other day, with no outline. Touch keeps today's 1 pt outline in the primary label colour.
- Width: N24.6's Size governs. Clarified 2026-11-24 (owner decision): Mac Home always uses 10 × 10 pt squares and a 2 pt gap. Increasing measured row content width reveals older whole week columns past 53 for as many as fit, with the newest week at the trailing edge and natural leftover space less than one 12 pt pitch. Touch keeps fixed squares and the grid's natural width, leading-aligned. No horizontal scrolling, date picker or legend.
- Text: one line under the grid in footnote secondary text, "312 reviews in the last 17 weeks", pluralised (AH8). It wraps and never truncates.
- No interaction: a tap does nothing. Accessibility: the grid and its line are one element, `home.activity.grid` (header `home.activity.header`), labelled "Review activity", with the summary line as its value. On Mac only, each square has a hover help tooltip with its date and count.
- Placement: a group of its own with the section header "Activity" and the side margins of the rows above it (never wider than the rows, never touching the edges). Deck page: after the Subdecks section, before the error line and the previous session. Today: after the decks with cards waiting. The tab layout's deck list (compact: iPhone, the Duo folded or held tall): after the deck rows. The deck list beside Today (Duo held wide) has none; Today has it. Mac: in the detail column, at the same places.
- Scope: the whole collection on Today and the deck list; the deck and its subdecks on a deck page (AH4).
- Hidden completely while the days shown have no reviews (AH8). It collapses only after the first result says so.
- Performance rules:
  - The page never waits for the heatmap. It loads asynchronously, off the main actor, on the query lane. Its height comes from seven rows of squares (N24.6's Size), reserved before data arrives. On Mac Home the seven rows always reserve 82 pt, including before width measurement. Returning to a page shows the cached grid at once while a due refresh runs in the background, without flicker.
  - One query per scope fetches all 53 weeks on touch; Mac Home uses a sparse all-history read with the same day bounds and home-deck filtering, missing days implicitly zero. No dense allocation extends back to an ancient timestamp. A width change (rotation, window resize, fold) only slices the cached result and updates geometry, labels and totals; it never queries again.
- Not in this build: streaks, statistics, tapping a day, month labels, legend, scrolling back, colour choices, settings, the Home Screen widget.
- Replaces N11.5's Activity bullet for the heatmap only; its studied-today line and streak stats are unchanged by this section.

## N15. Round 2 Settings and sync decisions (2026-11-19)

Decided by Antoine in chat and page comments on 2026-11-19 (`decisions.md`: `settings.root`, `settings.accountSync`, `sync.connect`, `sync.status`, `sync.serviceSwitch`). Where earlier text differs, this section wins. The behaviour is `sync/policy` P25, `sync/cloudkit-recovery` V27 and `sync/meguri-provider` MP6; all text is `sync/status-strings` T19. Those spec sections were approved on 2026-11-19 (`docs/process/review-log/redesign-specs-20261119.md`). The points raised for the owner are in P25.6, V27.9 and MP6.8; two of them (no device backup of a replaced iCloud collection, and Cancel removing the Meguri Sync sign-in) were decided on 2026-11-19 and are marked there. The lines marked "Changed 2026-11-21 (resume fix)" are in force: decided by Claude (coordinator) under Antoine's delegation of 2026-11-21 and his permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/sync-media-resume-20261121.md`).

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
3. **Status**: the status symbol, title and detail of T19.4 or T19.5. A needs you status has its one action as a button in this row. An issues row under it pushes the issues list. A waiting status has no button. Changed 2026-11-21 (resume fix): a waiting status whose T19.5 row has its own action (Try Again on the acceptance-unknown rows and on the paused setup media step, T19.1) has that button in this row, and the status panel shows the same action (N23.3). Changed 2026-11-21 (resume fix): while work runs, the status row shows the status panel's progress bar, with its value, under its text (N23.1, `sync/meguri-provider#MP6.5`); no row of its own holds a bar. The issues row shows only for an issue beside the state: Meguri Sync's media-blocked row shows it only while the files it lists are known (T19.5). One explanation: the status's title, its one sentence and its action; the Sync section repeats none of them in a paragraph of its own (Meguri Sync keeps only the server's files that have not reached this device, T18.6).
4. **Sync Automatically**: a `Toggle` with its footer, shown while a service is selected. Until the service is connected (selected, with its setup completed: a ready binding, `sync/policy` P25.3) it is disabled and shows its stored value (decided 2026-11-19 (review F8)).
5. **Sync Now**: a button, through `syncNowAvailability()` (N11.8); disabled with its reason as N11.8 says.
6. **Troubleshooting**: a `NavigationLink`, shown while a service is selected.

- While the value is Off the section has the Service row and its footer only.
- Changed 2026-11-21 (resume fix): while setup's media step runs, the reason under the Service control says the change waits for the media (`sync.switch.busy.media`, `.mediaDownload`), not for a review or a collection change.
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
- Changed 2026-11-21 (resume fix). On the Mac the page is pushed inside the Sync pane, as a page of a System Settings pane is: the Settings window keeps its section list, and the title bar shows the system back control at its leading edge with the page title beside it. The pane's own title stays at the leading edge before and after the page (N15.1). Exception (Mac, compatibility with the system's pushed-page header): the Settings window's toolbar style is set to the system's unified style. Need: SwiftUI gives a Settings scene the preference style, which centres the title, puts the back control in a row under it and moves the pane's title to the centre once a page was pushed; `.windowToolbarStyle(.unified)` on the scene has no effect. Verification: test-VM captures of `settings/sync/window/meguri-setup-media-paused` before and after a push and a pop, before the change (centred title, back control under it, pane title centred after the pop) and after it (back control leading, title beside it, section list kept, pane title leading after the pop), light and dark. Rows: the replace entries while their service offers them, Sync Log and Copy Debug Info; a paused, stopped or blocked media step's Try Again is the status row's (N15.2), and media blocked's Retry is in Affected Files (T19.5).

### N15.7 Changing the service, the first sign-in and the Mac Settings window (2026-11-22)

Changed 2026-11-22. Antoine's feedback on the built Mac app: the Turn Off Sync sheet "doesn't look good at all maybe we can redesign it?"; "It doesn't seem possible to resize this window on Mac."; and, on the Sync page before the first sign-in, "does it make sense to show the needs attention, sync automatically, sync now if you still haven't signed in at all… Think of what would be like the ideal page here." Decided by Claude (coordinator) under Antoine's delegation (`decisions.md`, 2026-11-22). Text: `sync/status-strings` T21. Where N15.2 or the `sync.serviceSwitch` and `settings.accountSync` screen contracts differ, this section wins.

- **The confirmation** (`sync.serviceSwitch`). Changing from a service to Off or to the other service asks once, with the system alert on every platform (on the Mac attached to the Settings window), as Delete Deck asks on the Mac (N20.7): one pattern on the Mac, iPad and iPhone (changed 2026-11-22 after the coordinator's review of the captures: iOS's confirmation dialog showed as a narrow popover from the Service row with no visible Cancel). No sheet, no grouped list of statements and no custom button styling. The title is a question (Turn Off Sync?, Switch to Meguri Sync?, Switch to iCloud?). The message is one paragraph saying what happens, the sign-in removal folded into it when leaving Meguri Sync, then what the old service has not received: a short "Not yet sent to …" list, one bulleted line per count this device knows (changes, review answers, media files), then one sentence; or one quiet line when everything has been sent; or one sentence when Meguri cannot check. It names only the services involved. Buttons: Cancel with the cancel role (Esc) and the destructive button with the system destructive role, never the default, in the positions the system gives them. The confirmation opens only while the change can run; otherwise the reason stays under the Service control. Progress and a failure show under the Service control after the confirmation closes. Why the alert also holds the list, rather than a sheet: the list is at most three short lines, which the system alert holds, and Delete Deck's confirmed losses use the same alert. Verification: test-VM captures of the seven `settings/sync/window/switch-*` examples, Mac, iPad and iPhone, light and dark, before and after.
- **Before the first sign-in** (`settings.accountSync`, Meguri Sync "setup not finished", T21.2). The Sync section shows only: the Service control in its own group; a group with Server Address, Username, Password and Sign In, the setup footnote as its footer and any sign-in error under the fields; and Troubleshooting. No status row, no Sync Automatically and no Sync Now: they appear once a sign-in succeeds. The fields look like fields: placeholders (an example address, Required), the URL, username and password content types so the password manager can fill them, Return moving to the next field and submitting from Password, and on the Mac focus starting in the first empty field when the page opens (on iPhone and iPad the page also holds Collection, and a keyboard raised on arrival would cover it). Sign In is at the trailing edge of its row on the Mac and a button row of the form on iPhone and iPad; it is never a disabled prominent button: the standard style while the fields are incomplete, prominent only when it can sign in. The sync button stays quiet: Sync Off's secondary symbol, with the title Not Set Up and its sentence as its help tag and accessibility value; the needs-you symbol stays for something that worked and now needs the person (a sign-in the server refused after it had worked, a choice to make, an upload to finish). A first Sign In that fails shows its error under the fields and the form stays. Leaving Meguri Sync in this state asks nothing. iCloud selected and not set up keeps its T19.4 row 16 as built (iCloud work is paused).
- **The Mac Settings window**: one width, a height the person can change (a minimum, and a default that holds the Sync section with or without a sign-in when the display is tall enough), as in the system Settings app; the system keeps the frame under the window's autosave name. The content has a fixed width and an open height, which the Settings scene's own resizing rule (`contentSize`, its default) reads as its limits. Exception (Mac, the system Settings app's behaviour): the window's resizable style is added, and the first open after this change takes the default height once, from the same window hook that sets the unified toolbar style (N15.5). Need: SwiftUI gives a Settings scene's window no resizable style even when its content's height is open, and opens it at the minimum height. Verification: test-VM captures of `settings/sync/window/*`, whose preview route reports the window's style, limits and size; before the change the window reported not resizable with its height fixed, after it resizable with only the width fixed.

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

- One thin grey bar at the bottom, below the answer buttons, on every device: the system's linear progress indicator with the secondary colour as its tint (on the Mac a drawn bar, the exception below). No text, no counts, no accent colour.
- Touch: the bar spans the screen's width on its very bottom edge, under the home indicator, which lies over its middle (restored 2026-11-22 by Antoine: "The progress bar is not fully against the bottom of the screen"; for part of that day it sat above the home indicator). The answer buttons sit just clear of the home indicator, at the bottom safe area's edge, so the indicator's strip is the only space between them and the bar; with no bottom inset, 8 pt and the bar stay free. iPhone Duo: the same edge folded, held wide and held tall; held wide it runs under both halves and across the fold. It is not a control, so it may cross (R9 covers controls).
- Mac: one standard spacing (8 pt) between the answer buttons and the bar (changed 2026-11-22, N16.9; it was 16 pt).
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
- With a fold through the row (iPhone Duo held wide) it keeps the size it has there, one column of the split row, and is centred in the row, under the card, across the fold. Exception to R9, decided by Antoine on 2026-11-22 ("Not aligned", and a much wider button would look odd to him (his words were relayed by the coordinator)): the button's label sits on the fold. Accepted because the card's own content is already centred there and tap zones make the button secondary. After reveal the four answers still split at the fold. Replaced (2026-11-19 text): "it keeps the trailing side, over Good and Easy: a centred button would sit on the fold (R9)".
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
- Space and Return/Enter invoke the same Done action once, with ordinary review focus. A held key or the last grade's event cannot dismiss the newly shown summary; text input, menus and other presentations keep their keys (authorized 2026-11-25).
- N11.6's summary sheet over the deck workspace is unchanged.
- Replaces: the check mark, "Finished for Now" as the finished title and its recorded-answers line.

### N16.6 Waiting, paused and notice states

- Review's states are plain text, centred in N16.4's column: a title, an optional message in secondary text and standard bordered buttons. No symbol.
- Waiting for a learning card that is not ready: "Waiting for Learning", "Your next learning card is not ready yet." and Check Now.
- Paused by an error: "Review Paused", the message and Try Again. Ended: "Session Ended".
- Loading shows the system progress indicator alone, and only for a start slower than Study's wait (N16.8, changed 2026-11-21; it had a "Preparing review…" label).
- A notice above the card keeps its warning symbol and text, centred across the review's width (N16.4); a long one still scrolls within its limit.

### N16.7 No session timer

- Review has no on-screen timer on any platform, and nothing in Meguri offers one. The timer view, its presentation model, strings and tests are removed.
- The preset's stored fields for showing the timer and stopping it on the answer stay in the collection untouched, for other Anki clients. The app's review bridge no longer reads them.
- Answer time is still measured and saved with each answer, up to the preset's maximum answer seconds.
- Replaces: R364's on-screen timer in N11.1 and N13.3.
- Specs aligned 2026-11-19: `review/timers` T2 and `review/reviewer-strings` S17 are removed, and Deck Options shows neither timer option (`sched/deck-options` O3).

### N16.8 Study starts on the card

Decided by Antoine on 2026-11-21 ("When you click on Study, there's a screen that shows 'Preparing review' for a very short moment … I would like to remove that screen … but also to make the Study click instant"; `decisions.md`).

- Study keeps the screen it was clicked on until the session's first card is ready, then the review appears with that card on it: no screen between the two, in every layout and for every Study entry point (the deck page, Today, the deck menu, the Deck menu bar command, the deck list's keys and double-click). The Study button keeps its normal look meanwhile; a second click does nothing.
- Only when the start takes longer than 500 ms does the review appear before its card, with the system progress indicator alone in the card area: no title, no "Preparing review…". It shows for a slow start, such as FSRS being turned on (`sched/app-coordination` F5.1), and then the card replaces it. A start that fails or is refused within the wait never shows the review: the deck page keeps its error, or the F5.3 alert shows; one that fails after the indicator showed closes the review again, as before. Going elsewhere during the wait ends the session without showing it, as choosing another row ends an open review (R22).
- Meguri keeps one set of card pages loaded while a collection is open and no review runs, beneath the window's content and never seen, and prepares in it the first card of the deck the visible Study targets (the deck page's deck; on Today, the first deck the walk starts). Media that arrives drops the prepared card, and Study's own deck selection keeps it. It reads only: no session starts, no deck is selected and no count or undo step changes until Study is clicked. No audio plays and nothing counts as shown until the card is on screen. The session uses the prepared card only when its own first card is the same card in the same state with the same content; otherwise it prepares that card as before. The pages are given up under memory pressure, in the background and when the collection closes.
- VoiceOver lands on the review as it does today; there is no loading announcement for a screen that does not appear. Reduce Motion keeps the system's own transition.
- Replaces: N16.6's "Loading keeps the system progress indicator with its label", and the review's loading screen with "Preparing review…".

### N16.9 The card runs on under the answers

Decided by the coordinator under Antoine's delegation on 2026-11-22, from his capture of a long card on the folded Duo ("there is a lot of room between the four buttons and the progress bar at the bottom… this could be reduced"; he also expected the whole card to continue behind the four buttons, which cut his last line; `decisions.md`).

- The card's page reaches the window's bottom edge on every platform. The answer area (Show Answer or the four answers, then the progress bar) floats over it as a system bottom bar (`safeAreaBar`), with no opaque band of its own: the card's own background and night mode show around and under the buttons. Each page's scroll view draws the system's bottom edge effect in its soft style only: no line at the answer area's top edge in any state or on any platform (Antoine, 2026-11-22: "What do you think of just not having that bar? I would prefer"), so the area never looks like a separate band or changes as the card scrolls. The hard style, which the automatic one resolved to, draws that line and a frosted band the height of the answer area; the soft one fades the card only near the bottom edge. So each answer button and Show Answer stands on the card page's colour, clipped to its own rounded rectangle, under its grey fill: card text never shows through a button, the label keeps its contrast, and over a plain card the button looks exactly as before; the page colour has its increased-contrast variants and needs nothing for Reduce Transparency. A blurring system material was tried first and rejected: the system fill turns vibrant over it, so the buttons came out lighter in light mode and darker in dark. On a card with its own background colour the buttons show the page colour rather than a tint of the card's (coordinator's decision under Antoine's delegation, 2026-11-22, pending his look; not Liquid Glass buttons and not the line). The shapes, sizes, tints and label colours are unchanged. The buttons keep their standard shapes and contrast.
- The page is told how much of its bottom the answer area covers (`WKWebView.obscuredContentInsets`, and on touch the scroll view's bottom content inset), so the last line of any card scrolls fully above the buttons, and a card shorter than the window is not covered at rest. WebKit keeps a card's fixed and sticky elements clear of that part.
- A tap or click in the answer area that misses its buttons does nothing on either side: the area takes it, so it reaches neither the card's page nor the tap zones, and no grade comes from a near miss of a button (U180). A drag that starts there does not scroll the card.
- Replaces: the answer row's band of the page colour between the card and the bottom edge (N3's reserved region now lies over the card, still the same height on both sides, so reveal moves nothing).

### N16.10 No scroll indicator on the card (touch)

Decided by the coordinator on 2026-11-23, pending Antoine's look. He showed the review on his opened Duo: the card's vertical scroll indicator ran as a long grey bar between the card and the column of answer buttons, which he found "a bit strange" and "web-like".

- On iPhone, iPad and a foldable, folded or opened, the review card's page shows no scroll indicator, vertical or horizontal, on either side. That includes the brief one the system shows when a long card appears. Built: the page's scroll view (`CardPage`, `showsVerticalScrollIndicator` and `showsHorizontalScrollIndicator` off).
- Nothing else changes: how the card scrolls (by touch, keyboard or VoiceOver), N16.9's answer area over the card, and N16.1's progress bar.
- An element of the card that scrolls on its own keeps whatever the card's styling gives it: that is the person's card, not Meguri's chrome.
- The Mac keeps the system's behaviour, which follows its "Show scroll bars" setting.
- Only the review's card pages and their warm-up use this page. Note Types and Browse have no card web view today. A later card preview decides its indicators for itself.

### N16.11 The leech notice is an icon (2026-11-23)

Decided by the coordinator on 2026-11-23, pending Antoine's look. He showed "Card was a leech." drawn as a banner row above the card and asked for one yellow icon in the top controls instead. It takes no extra height, is less prominent, and can be pressed when needed.

- **Placement:**
  - No banner row: the card keeps that height on every platform.
  - While the notice applies (`review/reviewer-strings#S3`), the review's top-trailing controls start with a button showing `exclamationmark.triangle.fill` in the symbol's own multicolour rendering, the system's yellow triangle. In the iOS glass toolbar group the multicolour symbol alone is drawn as a template (black in light, white in dark), so the icon also takes the system yellow as its style and tint; Increase Contrast's darker yellow applies there by itself (2026-11-23): `[leech][undo][more]` on touch, with the opened foldable's side column in the same order. On the Mac it is the first trailing toolbar item.
  - When no notice applies the button is absent and the group is as before.
- **Lifetime** (Antoine, 2026-11-23: it "should just stay visible for as long as you are on the card"; this replaces the 2-second expiry #166 gave the notice, and the earlier "as long as the notice" line):
  - It appears when the card after the leech answer is presented and stays through that whole card: question side, reveal, answer side, scrolling, the popover, audio and the More menu.
  - It goes when that card is answered (the next card arrives), buried, suspended, or taken back by Undo (or Redo), when another card is presented in its place, when the session ends or is left, or when another notice takes the area. A newer leech answer shows the newer notice. It never reaches the card after, nor the end-of-session screen.
  - VoiceOver announces it once when it appears, as #166 made it, and never again while it stays.
  - No timer is left for this notice: #166's 2-second dismissal covered only the leech notice and is removed.
- **The press** (changed 2026-11-25, Antoine in chat; first form 2026-11-23): a popover titled "Leech" beside the yellow triangle, on every width: at compact width it stays a popover sized to its text (changed 2026-11-23 after the captures; a medium sheet left the text floating in half an empty screen). The first form was one paragraph ("A card you answered in this session has been forgotten many times: it reached its deck's leech threshold…"): it did not say which card, read as if it were about the card on screen, and covered that card. Now:
  - The answered card leads: its question as one plain-text line (`browser/columns` C16, at most two lines, absent until it is read or when the card has none), then "Previous card · forgotten 8 times" in the secondary style. The count is the lapse count the answer gave the card; without one the line is "Previous card".
  - Under a divider, one secondary line says what the answer did: "Tagged “leech” so you can rework it later.", or "Tagged “leech” and suspended until you unsuspend it." when the answer suspended the card (`sched/leeches` L1 to L3, L5's `suspended` flag).
  - **Show in Browse**, a plain system button, only where Browse is shown (the Mac today; the iPhone and iPad get it with their Browse, U278, U279). It closes the popover, leaves the review as Back to Decks does (Edit Note's question included; Keep Editing stays in the review), and opens Browse on that card (`cid:<id>`), from Browse's root and top. The text never names Browse where it is not shown.
- **Not built: Edit Note and Suspend Card in the popover.** The leech card is the one just answered. The review's existing commands act on the card on screen, the next one, so they would edit or suspend the wrong card. Show in Browse is the way to both.
- **Accessibility:** label "Leech", hint "Shows which card it was."
- **Other notices** (a refused or failed answer, media, a changed card, the session record) keep their banner row unchanged.

### N16.12 The card scrolls on under the top bars (touch, 2026-11-25)

Asked by Antoine on 2026-11-25, from his capture of a long card whose first line was cut at a band under the screen's top edge: he agreed the card should not start flush with the top of the screen, and wanted a long one to "go up to the edge the screen" once he scrolls (`decisions.md`).

- On iPhone, iPad and a foldable, in every position, the card's page reaches the screen's top edge, under the status area and the review's top controls, as it reaches the bottom edge under the answers (N16.9).
- At rest the card starts where it did: below the top bars, so its first line is clear of them. The page is told how much of its top they cover (`WKWebView.obscuredContentInsets`, and the scroll view's top content inset). Scrolling moves the card on under them to the edge; no band clips it. WebKit keeps a card's fixed and sticky elements clear of that part.
- The scroll view draws no edge effect at the top: no blur, no fade, no line. The card stays sharp up to the screen's edge, behind the status area and the controls (Antoine, 2026-11-25, of the soft effect first built there: "can you remove that?"). The bottom keeps N16.9's soft effect.
- A notice's banner row (N16.6) stays above the card; the card then starts under that row and has no covered part at its top.
- The Mac is unchanged: the card sits under the window's toolbar.
- Replaces, on touch: "scrolls between the corner controls and the answer region" in N3's review rules, at the top end.

## N17. Round 5 editor and file transfer decisions (2026-11-19)

Decided by Antoine in chat on 2026-11-19 (`decisions.md`, rows `editor.addNote`, `collection.import`, `settings.importExport`; round 5 options 1B, 2B, 3B and the Duo answer). Where earlier text differs, this section wins. Media is never a separate choice in any of these flows.

### N17.1 Add Note

- One grouped form. Its first group has two rows, Deck then Type, each a system menu on its current value (`editor.add.deck`, `editor.add.type`). The deck list follows `editor/add-flow` A2; the deck starts as `notes/add-defaults` gives it. Choosing a type maps the content as A3 says and keeps the deck.
- Each field is a section of that form: its name is the header and one box sits below. The rounded name button and the rounded HTML button on every field are gone. A tap on the header still collapses or expands the field (`editor/field-html` F3).
- The fields stay in one web view, which keeps their scrolling, focus and caret (R5). The Deck and Type group sits above it and does not scroll; it is a real form sized to its two rows.
- One HTML control for the focused field, the `chevron.left.forwardslash.chevron.right` symbol labelled "HTML Editor", selected while that field shows its HTML. Touch: at the leading end of the key row, with Previous Field, Next Field and Done; the bar is a system toolbar that replaces WebKit's own. Mac: at the leading end of the panel's bottom bar, opposite Add. ⇧⌘X stays. The control takes the identifier of the field it acts on (`editor.field.<ord>.htmlToggle`), and `editor.add.htmlToggle` while no field has had focus.
- Add is disabled until the first field has content: typing counts at once, and a committed value follows the live field check. The live "Enter content in the first field." line is gone. The line still names a refused add (`editor/add-flow` A5 step 4), which now needs a race to reach.
- After an add the fields clear (sticky values stay), focus returns to the first field, and the navigation bar's subtitle reads "Added to <deck>", the deck's last name component; on Mac it sits below the fields (`editor.add.result`). It covers no field. VoiceOver announces it. It stays until the next note is started, the selection changes or the sheet closes. The sheet stays open.
- iPhone Duo opened wide, and any iPhone at regular width: Add Note covers the whole screen, with the form in a centred column no wider than the readable width. Folded or held tall: the sheet. iPad: the system page sheet. A fold or unfold while it is open moves the same draft and web view to the other presentation.
- Mac (authorized 2026-11-25): every Add Note/New Note entry opens one separate resizable native window, sharing Edit Note's columns form and window background. Note type and Deck are native pickers in the same columns as Edit Note's read-only facts; fields keep their 140 pt label column. HTML sits bottom leading, Cancel and Add bottom trailing. ⌘↩ adds and keeps the window ready for another note. The confirmation sits below the fields. Esc, ⌘W and the close button use the same held-flush/discard guard. The main workspace stays usable, and the collection owns the draft and runtime until the guarded close finishes. Mobile presentation is unchanged.
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

- Built 2026-11-24 (U162, deck package import; the implementer's reading, pending the coordinator and Antoine): File › Import… (⇧⌘I) and Settings › Collection's Import… open one system picker that offers deck packages, collection packages and zip files (`package/file-intake` K2.2), and the file decides the flow (P18): a whole collection goes to this section's flow with its replacement confirmation unchanged; a deck package opens the deck package sheet (`package/transfer-sheets` X2). Welcome's Import a Collection File keeps the collection-only picker (K6.1's creation path is not built). The deck package sheet is a grouped form in a sheet as tall as its content: its title bar names the file ("Import “<file name>”", the notes · cards line as its subtitle), Deck, Note Types and Media rows on one line each, the two progress switches with their help as the switch's second line, then "Existing Notes and Note Types" with Merge note types and the two update menus (not collapsed: the contract does not say, and a disclosure inside the form would nest a box); Cancel and Import (default) trailing. Progress replaces the form only once the work has run 350 ms. The merge confirmation replaces the form: title "Import “<file name>”?", its rows in red under "Deleted by This Import", the backup line; Cancel trailing and the confirm button at the leading edge on the Mac (a confirmation-placed button is the Mac's default button, which X2.3 forbids; the leading edge is where the Mac puts "Don't Save"), destructive with rows; with Meguri Sync's paragraph it reads Import and Upload. The result: "Imported “<file name>”", the found count, X2.5's lines as disclosure rows that list their notes in pages, with Show in Browse on the Mac, the non-zero counts below, Undo Import and Done (default). A drop on the main window draws an accent outline inside the window's edge while a file is over it.

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
- Changed 2026-11-22 (Antoine: "when the app is not focused some of the text of the homepage is greyed out but not consistently"). The app's and the window's focus never disable a row: in an inactive window the three rows look as they do in the key window, as the platform's rows do. Start Empty's "cannot start" means a flow, a sheet or sync work holds the collection, not focus. A click in an inactive window starts it as it acts on the other rows: the press waits up to half a second for the activation that click causes. While Start Empty waits for sync work (a service change not finished, or Meguri Sync working), which nothing else on the page shows, its explanation is replaced by "Unavailable while sync is busy. Set Up Sync shows why." in secondary text, not dimmed, and VoiceOver reads it as the hint; the symbol, title and chevron are dimmed. Route `home/deck-list/start-empty-waits`.
- The rows are buttons in a grouped form, at least the minimum target high (an unavailable row is the same row read as a dimmed button, N18.3's change of 2026-11-22). The chevron is drawn, because a row that acts in place gets none from the system. Title and explanation wrap at every text size. At the accessibility sizes the symbol and the chevron are left out and the text takes the row, as the state views leave out their symbols: a label there wraps each text around its symbol. VoiceOver reads the title as the row's label and the explanation (or the reason) as its hint; the title of the screen is a header.
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

- Sidebar layouts (Mac, iPad at regular width): a search field at the top of the sidebar, in the system's sidebar placement for `.searchable`, with the system's own "Search" prompt. It replaces the Go to Deck sheet. Changed 2026-11-22 (Antoine: "what do you think of just having it be 'Search'?"; decided by Claude (coordinator) under his delegation; `decisions.md`, row `home.goToDeck` of 2026-11-22): the prompt read "Search or Create a Deck". Creating a deck from the field is unchanged: the "Create deck named …" row follows the results while no deck has the typed name, and New Deck… stays in the Add menu and the File menu (⇧⌘N).
- Typing replaces the sidebar's list, the Today row included, with the results, as System Settings does. Each result is two lines: the deck's last name component over its parents in secondary text, joined with " › " (a top-level deck has no second line). Matching and order are the touch Search's (N14.2): `decks/deck-list` H6.1 and H6.2. Filtered decks are listed.
- After the results, while `decks/decks` D18 finds no deck with the typed name: a row with the `plus` symbol, "Create deck named “<query>”". It opens N20.3's New Deck form with the query as the Name and Parent Deck None.
- With no result and no create row, H6.3's one-line message shows.
- Return or a click opens a result: its deck page, with the sidebar selection on that deck (including the filtered deck page, `decks/filtered-study#F6`). The query and results stay while it is open, and the opened deck shows selected among them. Esc clears the field and the tree returns, the opened deck still selected.
- Mac: after each edit the first result is highlighted, or the create row when nothing is listed; ↑ and ↓ move the highlight over all rows and wrap (H6.3). The highlight does not change the detail; Return opens the highlighted row. iPad: a tap opens a result; Return in the field opens the first.
- ⌘F (Edit › Find…, N20.4), ⇧⌘O (a second key of Find…, with no menu item of its own) and "/" in the `decks.list` and `decks.page` contexts (H9.2; never while a text field or other view owns the key, N4) put the focus in the field, showing the sidebar first when it is hidden. In the tab layout the same keys present the deck list's Search (N14.2). During review Find… is disabled.
- Sidebar layouts' toolbar, on Today and the deck page: the sync status button and one Add menu (`plus`) with New Note (`square.and.pencil`) and New Deck… (`folder.badge.plus`). No separate Add Note, Go to Deck or Create Deck button. On a deck page New Note presets that deck, as Add Note does now (`notes/add-defaults`).
- "New Deck…" is the one name of that action in every menu; its form is titled "New Deck". "Create Deck" is no longer used.
- The tab layout keeps N14.2's Search button and Add menu, unchanged.
- The Go to Deck sheet is retired on every platform: no button, menu item or key opens it. Every list of deck search results shows the two-line rows, never a `::` path, and opens the deck page in every layout (N20.5).
- Replaces: N14.2's "The Go to Deck sheet stays for the sidebar layouts' toolbar, the "/" key and the Mac's Deck menu" and its "opens the Create Deck sheet" (now the New Deck form); N14.3's "Sidebar layouts keep Go to Deck and Create Deck in the window toolbar"; R26's sheet (its filter stays, H6.2); `decks/deck-list` H6.3's sheet, its highlight on opening and its Return that starts review, and H9.2's `/` and ⇧⌘O rows (now Find…); N18.1's Go to Deck sentence, which no longer has a screen.

### N20.2 The deck menu

- One menu per deck row: right-click on Mac, long press on touch. Its groups, in order:

| Group | Item | Symbol | Mac command key | Decks |
|---|---|---|---|---|
| 1 | Study | `play` | ⌘R | both kinds |
| 1 | Add Note | `square.and.pencil` | | normal |
| 1 | Browse Cards | `magnifyingglass` | | both kinds where Browse is available |
| 1 | Deck Statistics | `chart.bar.xaxis` | | both kinds where Statistics is available |
| 2 | Custom Study… | `slider.horizontal.3` | | normal |
| 2 | Create Filtered Deck… | `line.3.horizontal.decrease` | ⇧⌘F | normal |
| 2 | Edit Filtered Deck… | `pencil` | ⌥⌘O | filtered |
| 2 | Rebuild | `arrow.clockwise` | ⌥⌘R | filtered |
| 2 | Empty | `tray` | | filtered |
| 3 | Rename… | `pencil` | ⇧⌘R | both kinds |
| 3 | New Subdeck… | `folder.badge.plus` | | normal |
| 3 | Deck Options… (N20.9) | `gearshape` | ⌥⌘O | normal |
| 4 | Delete Deck (destructive role, N20.7) | `trash` | ⌘⌫ | both kinds |

- The normal deck page and finished state also offer Custom Study (`decks/filtered-study#F6`).
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

The menu bar, with N21.8's newer ordering: Meguri, File, Edit, View, Deck, Note, Format, Card, Window, Help. Round 6 originally had neither Note nor Format and placed Edit Note in Card; N21.8 moves Edit Note to Note and places Format between Note and Card. The Study menu is removed: there is one Undo and one Redo, in Edit. The Go to Deck… item is removed.

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
| Deck | Study | ⌘R | the shown normal or filtered deck has Study available (N14.3, H3) |
| Deck | Custom Study… | | a normal deck is shown and a filtered-study action is available, including on the finished deck page |
| Deck | Create Filtered Deck… | ⇧⌘F | a filtered-study action is available. In Browse, prefill the editor with its active card search. On a normal deck page, prefill the deck’s due-card search. Elsewhere open the editor with a blank search. |
| Deck | Rebuild | ⌥⌘R | a filtered deck is shown and a filtered-study action is available |
| Deck | Empty | | a filtered deck is shown and a filtered-study action is available |
| Deck | Rename… | ⇧⌘R | a deck is shown, filtered included |
| Deck | New Subdeck… | | a normal deck is shown |
| Deck | Deck Options… or Edit Filtered Deck… | ⌥⌘O | a normal deck is shown and its preset editor can open, or a filtered deck is shown and a filtered-study action is available. The title names the editor that will open (N20.9). |
| Deck | Delete Deck | ⌘⌫ | a deck is shown, filtered included, outside review, and no text field, sheet or panel owns the keys (N4, H9.2); N20.7 |
| Note | Edit Note | ⌘E | in review, a card is presented, on either side, and nothing else owns the keys (N20.8); in Browse, N21.8 |
| Card | Show Answer | Space | review shows a question side |
| Card | Again, Hard, Good, Easy | 1, 2, 3, 4 | review shows an answer side |
| Card | Bury Card | ⌘J | in review, as Edit Note (N20.8); in Browse, N21.8 |
| Card | Suspend Card | ⇧⌘J | in review, as Edit Note (N20.8); in Browse, N21.8 |
| Card | Back to Decks | Esc | during review |

- Edit keeps the system's text items intact: its Find submenu and every other item the system adds for text. Meguri's Find… is a separate item placed just before that group; while enabled it takes ⌘F, while disabled the system's Find ▸ Find… of a focused text view keeps it. ⇧⌘O is a hidden second key equivalent of Meguri's Find…, with no menu item; on iPad it is a key command with no discoverability title.
- Separators: File after New Deck…, Export Collection… and Sync Now; Edit after Redo, Paste and Meguri's Find…; Deck after Create Filtered Deck…, Empty and the options editor command. Round 6 placed Card separators after Show Answer, Easy, Suspend Card and Back to Decks; N21.8 gives the newer Note, Format and Card grouping and ordering. A separator with nothing after it yet is left out.
- A filtered-study action is available in the foreground main window when the collection is ready for a deck write, outside review, with no sheet or conflicting operation. The selected-deck commands act on the Home deck page shown in the sidebar or tab layout. Create Filtered Deck… also works without a selected deck, including from Today and from Browse on the Mac. It captures the active Browse search when invoked, regardless of which cards are selected (`decks/filtered-study#F1`, F6).
- Each command follows N4: disabled outside its context, routed through the action and guard of its visible control, and subject to input ownership. The Card menu's unmodified keys follow N11.3: the card page keeps the first claim. ⌘⌫ and the other menu key equivalents never take a key from a text input (H9.2). The one exception is Find…: while enabled it takes ⌘F even when a text input has focus, as its row above says.
- iPad with a keyboard: the same menus, items and keys in the iPad menu bar and in the list the system shows while ⌘ is held (SwiftUI `commands`), except the Card menu's single keys (Show Answer, the four ratings, Back to Decks): they carry no key equivalents on iPad, because the card host keeps the single key registration (N11.3's first claim), so the ⌘-held list does not list the review keys. Note › Edit Note ⌘E, Card › Bury Card ⌘J and Card › Suspend Card ⇧⌘J (N20.8, N21.8) are not single keys and keep their equivalents there. iPhone and the iPhone Duo have no menu bar: with a keyboard the same keys act, and where the system shows its ⌘-held list it follows these menus.
- Built 2026-11-24 (U162; the implementer's reading, pending the coordinator and Antoine): File › Import Collection… is "Import…" (⇧⌘I, same enablement), since its picker now takes deck packages too (N17.2's built line); the table's "Import Collection…" label described the collection-only picker. Settings › Collection's row reads "Import…" as well.
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
| Search | the sidebar search prompt (also N14.2's): the system's own default prompt, no string of ours | changed 2026-11-22: it read "Search or Create a Deck" |
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
| Edit Note | the Note menu (moved from Card by N21.8), the card's context menu (Mac), the touch review's More menu, the form's title | N20.8, N21.8 |

- No longer used: "Go to Deck…", "Create Deck", "Overview", "Deck Actions", "Deck name", "Rename Deck…", "Close Review", the Study menu's title and its "Undo Answer" and "Redo Answer" items (Edit's Undo still reads "Undo Answer Card" after an answer, from the core's step name), "Import…", "Export…".

### N20.7 Deleting a deck

Decided 2026-11-20 (`decisions.md`, `home.deckActions`, option 4B). The operation is `decks/decks` D8; its summary D8.5.

- Entries: the deck menu's Delete Deck (N20.2, final group, destructive role, `trash`; filtered decks too) and Deck › Delete Deck (⌘⌫, N20.4), both on that one deck. No other entry; no swipe action (R49).
- Delete Deck first reads D8.5's summary for the deck. With `cards` 0 the deck is deleted at once, with no question: an empty deck, and a filtered deck, whose cards return home (D9). With `cards` 1 or more Meguri asks first. After the user confirms, the summary is read again right before the delete; when it differs from the one the question showed and `cards` is 1 or more, Meguri asks again with the new counts instead of deleting. A delete that asked nothing reads it the same way, so a card added after a zero-card read is asked about. A new generation in between ends the delete with nothing written. Changed 2026-11-21 (confirmed losses; decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review, `docs/process/review-log/delete-deck-losses-20261121.md`): the delete passes the counts of the last summary read before it as its confirmed losses: when that recheck has `cards` 1 or more it equals the confirmed question's, since a differing recheck with cards asks again; when it has `cards` 0 (nothing was asked, or the cards left after a question) its own counts are passed (`decks/decks` D8 step 6), so an arrival between that read and the commit that raises a count is no longer deleted without the core refusing: the core refuses with `LossesChanged` and the new counts, writing nothing. The check is by count: when one card leaves and another arrives, the count stays equal and the arriving card is deleted without having been shown (`decks/decks` D8 step 6, Comparison). Meguri then treats those counts as a fresh read: with `cards` 1 or more it shows the question again, in the same place, with the new message line; with `cards` 0 it deletes at once with the new counts. So a card that raises `cards` is always shown before it is deleted, but a deck that raises `decks` is shown only when the refusal's `cards` is 1 or more: when it is 0 (an empty subdeck added after a zero-card read, or a card leaving as an empty subdeck arrives), the subdeck is deleted without a question, as an empty subdeck present at a zero-card read would be. That refusal shows no `Couldn’t Delete` alert.
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
- A delete that takes the backup of D8 step 4 shows nothing extra. A delete that fails writes nothing and shows an alert: the title `Couldn’t Delete “<name>”`, the message "Meguri couldn’t make a backup first, so nothing was deleted." for `SafetyBackupFailed` and "Nothing was deleted." for any other failure except `LossesChanged` (above; changed 2026-11-21, confirmed losses), and OK.
- While the delete runs, the deck's menu items and the Deck menu are disabled.
- Strings, all new: `Delete “%@”?`, the six messages above (a String Catalog entry with plural variations for S and C), "Delete Deck", "Delete" (Mac alert), "Cancel", `Couldn’t Delete “%@”`, "Meguri couldn’t make a backup first, so nothing was deleted.", "Nothing was deleted.", "OK", and Edit's "Undo Delete Deck" and "Redo Delete Deck" (R173's pattern).
- Replaces: R66's toast "Deleted <deck> (N cards)" with Undo (no toast); `home.deckMenu`'s "deletes … end with an Undo toast"; R67 stays as D8.5 states it.

### N20.8 Review actions: Edit Note, Bury Card, Suspend Card

Decided 2026-11-20 (`decisions.md`, `review.editNote`, option 7A; then `review.question`, round 7 options 1D and 2A, which replace 7A's separate Edit Note button). The review side of Edit Note is `review/edit-current`; Bury Card and Suspend Card are `review/card-actions` A4 and A5 on the current card. The card browser is not part of these rounds.

- **Touch toolbar.** Upper left Back to Decks (N11.1, N20.5). Upper right, leading to trailing: Undo; Redo right after it, only while a step can be redone (N11.1); then one More button, the system `ellipsis` symbol with the accessibility label and Voice Control name "More"; then the sync status button only while sync needs the person (N16.2). There is no separate Edit Note button.
- **The More menu** is a pull-down menu in groups, each item with its system symbol: [Edit Note (`pencil`)] | [Bury Card (`moon`), Suspend Card (`pause.circle`)]. Later card actions slot into these groups or new ones after them (`review/card-actions#A13`'s order: Card Info beside Edit Note; Flag and Mark Note; Set Due Date… and Forget…; Delete Note last and red); none is added now. Items are enabled while a card is presented, on either side, and disabled in the waiting, paused, finished and loading states and while an answer, an undo or another action runs (`review/commands#K1`).
- **iPhone Duo.** Back to Decks, Undo and More stack on the trailing edge, in that order from the top; the menu opens beside them. Opened flat, they and the menu stay on the trailing half, away from the hinge.
- **Mac toolbar menu (user-approved 2026-11-24).** Keep a visible, icon-only `ellipsis` SF Symbol at the upper right, after any leech notice and before the conditional sync status. Use a native SwiftUI `Menu`, with no disclosure chevron or visible text; its localized help and accessibility label are "Review actions". Reuse the touch More menu's items, grouping, actions and enablement: Edit Note | Bury Card, Suspend Card. Keep Back to Decks at the upper left and undo/redo in Edit. This replaces the earlier choice of no Mac toolbar control.
- **Existing menus and keyboard ownership.** Keep the menu bar and Mac card context menu. The Note menu shows Edit Note ⌘E; the Card menu shows Bury Card ⌘J and Suspend Card ⇧⌘J (N21.8). A secondary click on the card shows Edit Note | Bury Card, Suspend Card first, with these keys, above the system's text items (Look Up, Copy, Translate) when text is selected. Show the same shortcuts in the toolbar menu where the native API supports display without registering duplicate handlers. SwiftUI's `keyboardShortcut` also registers a handler, so the toolbar menu currently omits key equivalents to preserve N4's menu-bar guards. Do not add a key monitor or duplicate shortcut registrations. iPad with a keyboard keeps the same Note and Card menu items with their ⌘ keys in the menu bar and the ⌘-held list (N20.4: only the Card menu's single keys are left out of it). Touch More behavior and the `E`, `-` and `@` keys of `review/commands` stay unchanged.
- **Edit Note** opens round 5's Add Note form (N17.1) on the current card's note, titled "Edit Note", with Cancel and Done. Its Deck and Type group is replaced by the navigation bar's subtitle "<deck> · <note type>": the card's deck by its last name component, and the note type's name; neither can be changed here. The fields, the HTML control and the tags are N17.1's; the first field has focus. Placement, as Add Note's: iPhone, and the Duo folded or held tall, a sheet; the Duo held wide (any iPhone at regular width), the whole screen with the form in a centred column of the readable width; iPad, the system page sheet; Mac, its own window over the review window, which stays visible and waits (F1, 2026-11-25, below). A fold or unfold moves the same draft to the other presentation. Changes are a draft: Done saves them with one note update (`review/edit-current#E2`) and returns to the same card, rendered again, at the same place in the session (E3). Done with no change closes. Cancel, Esc, ⌘W and the swipe down close without saving; with a change they first ask "Discard changes?" with Discard (destructive) and Keep Editing. ⌘Return is Done on Mac.
- **Mac Edit Note window (user-approved 2026-11-25, F1; replaces the 2026-11-24 sheet, option A).** Edit Note opens in its own titled, resizable window over the review window, which stays visible behind it and waits: its card actions hold and Back to Decks is disabled while a note is being edited. One window at a time; Edit Note again brings it forward. The window's title names the note: its first field as plain text, else "Edit Note". It opens centred on the review window the first time and then remembers its frame. The form is AppKit's columns form on the window's own background, no grouped box: the note type and deck as plain secondary text in the label column's rows ("Note type:", "Deck:"), then each field's name with a colon right-aligned in a 140 pt column beside its bezelled text box, the system focus ring on the field being edited. A field's text is the system font at the labels' 13 pt, not the note type's field font, which belongs to the card (as in Browse's inspector). As in Browse's inspector, a `[sound:…]` reference shows as a chip that plays its file on a click, and an image as a thumbnail that opens in the default app; only files inside the collection's media folder are read, and only images open. The bottom row has the HTML control at the leading corner and Cancel beside Save (default, ⌘↩) at the trailing corner. Cancel, Esc, the close button and ⌘W close only this window, asking "Discard changes?" on it when there is a change (Keep Editing or Discard); Save saves and closes it. Either way the review's window takes the keys back on the same card. Closing the review's window cancels the same way. Not built: on/off switches for flag fields (a non-empty value, even "0", still creates the card, so an off switch could still leave the card in place).
- **Bury Card** hides the current card until tomorrow; **Suspend Card** hides it until it is unsuspended. Both act on the card, never on its note (`sched/manual-ops` M2, M3, one op naming the review session). The next card appears as after an answer (`review/card-actions#A4`), or the congratulations content when nothing is left. No banner and no toast: VoiceOver announces "Card Buried" or "Card Suspended".
- **Undo** covers the last change made on the review screen: an answer, Bury Card, Suspend Card or a saved Edit Note. Undoing a bury or a suspend puts that card back on screen on its question side. Redo keeps its rule (N11.1, iPhone Redo after Undo). The Undo control's label, Mac tooltip and Edit › Undo name the step in Apple's style (`review/reviewer-strings#S5`'s display names): "Undo Answer", "Undo Bury Card", "Undo Suspend Card", "Undo Edit Note"; VoiceOver hears "Answer undone", "Bury Card undone" and so on.
- **No unsuspend inside Meguri** (option 2A). There is no Suspended Cards list, no deck-menu item and no other way to remove a suspension in this version. Until the notes and cards browser arrives, a suspended card comes back only by Undo right away or by another client after a sync.
- Strings: "Review actions" (new 2026-11-24, Mac toolbar help and accessibility label), "More" (new), "Edit Note", "Bury Card", "Suspend Card" (exist, `review/reviewer-strings#S18`), "Card Buried" and "Card Suspended" (new, VoiceOver announcements), the step display names "Answer", "Bury Card", "Suspend Card", "Edit Note" (new, S5), "<deck> · <note type>" (new), "Done", "Cancel", "Discard changes?", "Discard", "Keep Editing" (exist).
- Replaces: round 6's touch Edit Note button (7A's `pencil` beside Undo); `review/card-actions` A4 and A5's toasts and `review/reviewer-strings` S4's `review.toast.cardsBuried` and `review.toast.cardSuspended` for Bury Card and Suspend Card (no toast); S5's raw step names in the Undo titles ("Undo Answer Card", "Undo Bury"); R394's iPad "trailing inspector" and its panel as U190's editor in edit mode; R397's Close button (now Cancel and Done); R824 and R863's Esc that saves, for this form; `screen-inventory.yaml`'s `review.menu` toasts.

### N20.9 Deck Options, first version

Decided 2026-11-20 (`decisions.md`, `deck.options`, option 6B). The scope, the save input and the strings are `sched/deck-options` O17 and `decks/deck-options-strings` K10. Extended 2026-11-25 at the user's request: Display Order and New Preset join this native page; the other Advanced sections remain deferred.

- On a normal deck, the row menu's Deck Options… (`gearshape`, group 3), Deck › Deck Options… (⌥⌘O), the O key in the deck list or page, and the finished state's Deck Options link open the preset form (N20.2, N20.4, H5.2 and H9.2). On a filtered deck, the row menu’s Edit Filtered Deck… in group 2, Deck › Edit Filtered Deck… (⌥⌘O) and O open the search definition (`decks/filtered-study#F1`). Deck menu commands are disabled during review. The review menu owns its separate card actions.
- One grouped form:
  1. no header: **Preset**, a menu picker of the collection's presets, each listed as `decks/deck-options-strings` K6 gives it ("Default (used by 6 decks)"); the row shows the chosen preset's name. Footer: "Used by N decks. Changes apply to all of them.", or "Used by this deck only." when N is 1, with N the count the picker shows (`sched/deck-options` O6.1).
     **New Preset…** sits beside the picker, stacking below it when width requires. It opens a native name form (Name, Cancel, Create) with a short footer explaining built-in defaults and the later Save. Blank or whitespace-only input cannot be submitted. The core draft supplies the canonical name, selected staged preset and defaults (O6.3); no collection write happens before Save. Staged presets show a use count of zero.
  2. **Daily Limits**: New Cards per Day, Maximum Reviews per Day, each a number field trailing in its row.
  3. **Memory**: Desired Retention, a percentage field with a stepper of 1 %. Footer: "How likely you are to remember a card when it comes back."
  4. A native **Advanced** disclosure beneath Memory, collapsed each time the page opens. It contains **Display Order** and O3's five preset menu pickers in O3's order, using K3/K5's exact labels and the core's actual selected values and choices. A localized footer explains gathering versus sorting. Controls adapt to compact width and accessibility text sizes; expanding the disclosure changes no option.
- When the deck holds its own value for a row (a This deck or active Today only limit, or its own desired retention; O2 item 3), the section's footer adds "This deck has its own limits, which stay in force." (Daily Limits) or "This deck has its own desired retention, which stays in force." (Memory). The page cannot change or remove them in this version.
- No limit tabs, no FSRS switch (N18.1), no Clone, Rename or Remove, no Save to All Subdecks, no restore-default or help controls. New Preset stages only; Save creates all staged presets and assigns the currently selected preset to this deck in O7's single operation, and Discard creates nothing. Pending edits and preset operations are serialized, with input and Save guarded during Add.
- Issues: O4's errors and warnings of the three rows show under their row as N11.9 says; an error disables Save.
- Placement, pushed like a page: compact (iPhone, the Duo folded or held tall, iPad at compact width) full screen with a back button; the Duo held wide, iPad and Mac in the detail area in place of the deck page. Touch: the navigation title "Deck Options" and Save as the confirmation action. Mac: the toolbar title "Deck Options" with the deck's path joined with " › " as its subtitle, the back button, and Save as the toolbar's confirmation (⌘S; ⌘↩ too, O15.3).
- Save runs O17's save; on success the page returns to the deck page. A refused save keeps the page with its issues.
- Leaving: while the draft differs (O6.6), Cancel replaces the back button and swipe back is off on every platform (N11.9's iPhone rule); Cancel, Esc (Mac) and choosing another sidebar row ask "Discard changes?" with Discard (destructive) and Keep Editing. With no change, back leaves at once.
- Strings (new unless marked): "Deck Options" (title; the undo step's name exists), "Deck Options…" (exists in R283), "Preset" (K4's tab text, here a row label), "Daily Limits" (K2), "New Cards per Day", "Maximum Reviews per Day", "Memory", "Desired Retention", "Used by %@ decks. Changes apply to all of them.", "Used by this deck only.", "How likely you are to remember a card when it comes back.", "This deck has its own limits, which stay in force.", "This deck has its own desired retention, which stays in force.", "Save", and the existing "Discard changes?", "Discard", "Keep Editing", "Cancel". Keys: K10.
- Replaces, for this version: N11.9's Preset Actions menu, Save to All Subdecks, restore-default and help buttons, the reload row, two columns and the review-time sheet (each comes back with the unit that brings its part); R736's Anki labels for these three options (6B: Apple title case, K10); R740's and R742's tabs; the screen contract `deckOptions.options`' layout until the Advanced page lands.

### N20.10 Selected-deck export (2026-11-26, pending native design review)

Export Deck… in the deck's context menu, the touch deck menu and the Mac File and Deck
menus opens a grouped form showing the full deck name and the Anki deck package format.
The action applies to the deck the menu names, including its subdecks and all cards of
each selected note.

Include Scheduling Information and Include Deck Presets start off. Include Media starts
on. A completed preparation remembers the three choices on this device for the next
deck export. Export opens the native destination flow of N17.3 with a deck-named
`.apkg` file. Cancel closes the options form.

## N21. Round 8: Browse (2026-11-20)

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, rows `browser` and `home.deckList`; round 8 answers 1A 2A 3A 4B 5D 6A 7A 8A 9A 10A 11A, with the forgetting curve in the first version, system symbols on the Today and Browse rows, every bulk action in the first version, a Note menu and ⇧⌘B). Where earlier text differs, this section wins. Behaviour is `browser/workspace` and `search/filter-tokens`; this section is the layout. Approved with those specs by Antoine on 2026-11-20. Strings: `browser/workspace` V16 and `search/filter-tokens` T15.

### N21.1 Where Browse lives (11A)

- Mac and iPad: the sidebar always starts with Today (`calendar`) and Browse (`magnifyingglass`), each with its system symbol; deck rows keep text only. Selecting Browse turns the rest of the sidebar into the filters (`browser/workspace` V6); selecting Today shows the decks again. One window, three panes: sidebar, table, inspector. No separate Browse window.
- iPhone and the Duo folded or held tall: a third tab, Browse, between Decks and Settings; on the Duo, the middle symbol of the side tab bar (`browser/workspace` V14.3).
- Mac: View › Browse ⇧⌘B. The Browse row returns to Browse's kept state (V1).
- Changed 2026-11-24 (Antoine in chat, sidebar option 1 of the search and sidebar mock; `decisions.md`, row `browse.browser` of 2026-11-24): with Browse selected, every filter row leads with a symbol in the secondary colour: `magnifyingglass` for a saved search, `rectangle.stack` for a deck, `doc.text` for a note type, `tag` for a tag; a card state has its own (New, Learning and Review a filled circle in the deck counts' colours, Suspended `pause.circle` and Buried `moon` as the table's state column draws them); a flag `flag.fill` in its colour, No Flag `flag.slash`. This replaces "deck rows keep text only" for Browse's sidebar; Today's deck rows keep text only. The list's own selection is the Browse row alone. A filter row the query selects (`search/filter-tokens` T9) draws itself: its name in the emphasized weight and a rounded background in the accent colour's quaternary tint. The checkmark first built at the trailing edge went on 2026-11-25: an active filter row shows none, because its background already says it (Antoine's pick; `decisions.md`, row `browse.browser` of 2026-11-25). Exception to the native contract, for a functional need: the system draws every selected row of a sidebar list alike, so the section shown and each active filter were all solid accent rows with nothing to tell the place from the filters. The rows stay the system list's rows, with its badge, disclosure and context menu; VoiceOver reads an active filter row as selected. The arrow keys move the list's selection through the rows without changing the query, and Return or Space is a click on the row they stand on. Note Types, with more than 12 note types, shows the 8 with the most rows in the mode and those the query selects, then "Show All <N>…" (`browser/workspace` V6). The sidebar's own search field stays: the mock removed it, but Today's sidebar keeps its field (N20.1), so the rows would jump between the two sections.
- Round 10 (`decisions.md` `home.today`, 2026-11-20) makes Today a summary and gives deck rows waiting counts and Return, double-click and S to start a deck. Those apply with Today selected. With Browse selected the deck rows are filters: a click or Return sets the Deck token, S and double-click do nothing, and the number is Browse's count of the cards the value finds (T12.4).

### N21.2 The search field (2A)

- In the window toolbar, after the title and the Cards / Notes control (N21.3), as wide as the toolbar allows; on touch at the top of the list.
- Tokens are system search tokens (`searchable` with tokens) where the system's token view supports a token menu and the keys of T10; if it does not, the token view is a custom view, recorded as this screen's exception (need: T10's per-token menu and ⌘-click; verified against the system field's look in the VM screenshots). Each token shows the kind label in the secondary style then the values; an **is not** token adds "is not" after the kind; an advanced token is grey and shows its text. A flag value shows its colour dot.
- Suggestions are the system's search suggestions list, in sections (T12), each row with its symbol, label, secondary detail and trailing count.
- No syntax mode, no syntax reference, no error line: the field never shows Anki syntax errors (T5). The prompt is "Search".
- Esc closes the suggestions, then empties the query. The field's clear button empties it.
- Changed 2026-11-24, Mac only (Antoine in chat, search option B of the search and sidebar mock; `decisions.md`, row `browse.browser` of 2026-11-24): the toolbar's field holds the text only, and the query's tokens show in a **filter bar** over the table, across its width, on as many lines as they need. Each token is a system menu button with a capsule border: the kind in the secondary colour, "is not" after it when negated, then the values; a flag's colour dot; an advanced token's text in the secondary colour. Its menu is the token's (`search/filter-tokens` T10). After the tokens, Filter (`plus`) lists the kinds; choosing one puts the focus in the search field with that kind's value list as its suggestions. At the trailing edge, Save Search… (T13) and Clear, which empties the query. The bar shows while the query has a token, also over the no-results state; with no token there is no bar. A suggestion accepted in the field becomes a token in the bar. The field's clear button and Esc empty its text; Clear empties the query (a second Esc does not on the Mac: the field holds no token to clear, and the bar's filters, most of them set from the sidebar, are not what Esc cancels. Decided 2026-11-25 by Claude under Antoine's delegation, pending his look; `decisions.md`). Exception to the native contract, for a functional need: macOS fixes the toolbar search field's width (N21.3, `decisions.md` 2026-11-22), so its own tokens scrolled out of sight after the second and a query of several filters could not be read or edited. The field, its prompt and its suggestions stay the system's. Touch keeps the tokens in the field.

### N21.3 Mac layout (1A, 3A, 4B)

- Toolbar, leading to trailing: the title "Browse", the Cards / Notes segmented control, the search field, the Add menu (round 6's), the inspector toggle (`sidebar.trailing`). Nothing else: no action buttons (7A).
- Table: a native `Table` with sortable, resizable, reorderable columns; the state column first (V2.5); text leading, numbers and dates trailing, tabular figures; system selection; no row tints, no alternating colours beyond the system's.
- Status line under the table in the footnote style: V11's text at the leading edge, the column set menu at the trailing edge.
- Under about 900 points the inspector hides itself first; Return or a double click on a row shows it over the table.
- Built 2026-11-23 (U275 follow-up, VM probe of every placement on macOS 27; pending the coordinator and Antoine): the decided order cannot be had with the system's search field and its suggestions together. A `.navigation` item is drawn before the title, and a `DefaultToolbarItem(kind: .search)` does not move the field. With `.toolbar` or `.automatic` placement macOS draws the search field last, after the inspector toggle. With `.toolbarPrincipal` placement and Cards / Notes and the sync button as `.secondaryAction` items, the order is the decided one (title, Cards / Notes, sync, a wide search field, Add, inspector), but the field then shows no search suggestions (T12). Built: the title, Cards / Notes (`.secondaryAction`, drawn right after the title), the sync button, the Add menu, the inspector toggle, then the search field at the trailing end with its suggestions. The UI suite asserts this order by frames. A custom field is not used (N21.2's system field).
- Built 2026-11-24 (U272; the implementer's reading, pending the coordinator): the column chooser is the table header's context menu (a secondary click on the header) and View › Columns, one list for both: every column of the mode with a check, the set's columns first in their order (field columns included), then the others by label; Fields › with each note type's fields under its name as a section; in a note type's own set its first 8 fields, "<N> More Fields ›" and Standard ›; then Size Columns to Fit and Reset to Standard Columns (Use Standard Columns in an own set). The last column's item is disabled. Exception to the native contract, for that functional need: SwiftUI's `Table` offers a header menu only with `TableColumnCustomization`, whose order and widths are an opaque value of its own with no Fields submenu, while V2.4 keeps the order in the collection; so the header's `NSMenu` is AppKit's, on the table SwiftUI draws (verified in the test VM, light and dark). A click on a sortable header sorts (verified). Not confirmed: dragging a header to reorder the set and dragging an edge to keep a width (the handlers are in place: the table view's column-move and column-resize notifications and a local monitor of double clicks on a header edge), since XCTest's drag moved no header in the test VM (`docs/process/verification-status.md`). A change shows at once on the rows already read; a column added draws nothing until its rows are read again. "Reset to Standard Columns" in the standard set (V2.3's defaults for the mode shown, and this device's widths; the sort stays) is not in V2.2, which gives the standard set's status-line menu no item: added on the coordinator's brief. The status line's set name is a menu in every set: Reset to Standard Columns in the standard set, V2.2's two items in an own set. Not built: the iPad's touch-and-hold header menu (U278).

### N21.4 The inspector (6A)

- Browse shows imported fields in stored ordinal order, including layouts with gaps (`notetypes/records` N1). A field's position in that list selects the note value to display or edit. Saving verifies the stored layout and preserves untouched fields. A note whose field count differs from its definition shows the counts and directs the user to Check Database.

- Trailing pane with a segmented control Note | Info at the trailing edge of its bottom bar (owner decision 2026-11-25, below; first at its top).
- Note: the card preview (folds with its header), Deck and Type rows as round 5's form, the filled fields as labelled boxes, one "<n> Empty Fields" disclosure row, Tags as a token field, and a formatting bar under the fields (Bold, Italic, Underline, Cloze when it applies, Attach, HTML).
- Info: the stats rows, Review History, then the Forgetting Curve chart (Swift Charts, system style), as `cardInfo.card` draws them.
- Mac Tags completion (2026-11-25, owner request): Browse uses the public macOS `NSTextField.suggestionsDelegate` / `NSTextSuggestionsDelegate<String>` presentation inherited by `NSTokenField`, with plain suggestion items and system sizing, selection, keyboard, pointer and dismissal behavior; suggestions match case-insensitive component prefixes, exclude held tags and replace only the active tag on acceptance, preserving adjacent tokens and native editing-end saving. Native build, light/dark VM captures and design acceptance remain pending integration verification.
- Built 2026-11-23 (U273; the implementer's reading, pending the coordinator): the pane is the system inspector (`.inspector`, 280 to 460 points, 320 ideal), shown or hidden by the toolbar's `sidebar.trailing` toggle, the last trailing item, and by View › Show Inspector / Hide Inspector ⌃⌘I (the key of the system's `InspectorCommands`; ⌥⌘I stays Card Info). Whether it shows, whether the preview is folded and which note types show their empty fields are kept on the device. Under 900 points of window width it hides itself (it saves first); Return or a double click on a row (the table's primary action) then shows it over the table, at 320 points, with a separator at its leading edge, and Esc or the toggle hides it again; wider again, the person's own choice shows. The plan gives U273 the Note tab only, so the Note | Info control and the Info tab come with U274; until then the pane has no tab control and View › Card Info stays disabled. No row: "No Selection" (content-unavailable). Several rows: N21.5. One row (Option A, the owner's choice of 2026-11-23, `decisions.md`): the "Preview" header stays at the top (it folds the preview; a Play Sound control when the card has sounds and "Card <i> of <k> ›" at its trailing edge when the note has several cards); under it one scroll view holds, from top to bottom, the preview (160 points tall), Deck and Type as label and pop-up rows (labels right-aligned in a short leading column, the two pop-ups the same width, filling the row), the fields as a plain list in the round 5 editor's page in its inspector variant (each field's name on its own line in 11-point secondary text that wraps, the text under it in the system font at the body size, full width, wrapping, in the content's own direction unless the field is right to left; no box and no background until the field is edited, when it takes the text background and the focus ring; a hairline separator between fields; the note type's fonts stay in the preview), "<e> Empty Fields" as a disclosure row under them, and a Tags section with its own small header over the system token field, which wraps; the formatting bar stays pinned at the pane's bottom edge under a separator. The page grows with its fields and never scrolls itself (no scroll bar inside a field); the scroll view keeps the caret's line in sight while typing. A sound reference shows as a chip (a note symbol and the file name, cut in the middle) that plays on a click; an image shows as a thumbnail at most 64 points tall that opens in the system's viewer on a click; both are presentation only (the chip writes back its reference's exact text) and the raw text is behind the source toggle. The review's Edit Note sheet keeps round 5's form (N17.1, N20.8): it does not share the inspector variant. The preview is review's own page pair (the note type's CSS as the user wrote it), rendered after the fields show and again after each save, not on each keystroke: the stored note is what renders (`browser/preview` gives no render of unsaved text). A click flips question and answer (a click recognizer on the card, which still passes the click to links and play buttons); audio never plays by itself. Show Both Sides (K6.3) is not built: writing `previewBothSides` is a K10 non-undoable write that would empty the undo history, and Browse has no core write for it yet. The Type row is a pop-up menu on the note type's name: the other note types are listed disabled (Change Note Type… comes with U284), then Edit Note Type…. Choosing another deck runs Change Deck on the shown row's cards after a save, one "Change Deck" step. The formatting bar: Bold, Italic, Underline, Cloze Deletion (cloze note types only), Attach (in its place, disabled until the editor's media insertion, U194), then at the trailing edge the HTML control; each acts on the focused rich field and is dimmed otherwise; the source toggle too is dimmed until a field has the focus. Images in a field show as the editor shows them today (the page loads no media yet, U194); the preview shows them. Saving reads V12 over `editor/edit-flows` W3: a field's 600 ms idle commit only updates the draft; the note is written when a field loses the focus, before another row's note shows, when the pane hides, when Browse is left and when the app goes to the background, and only with what differs from what is stored. The core's `engine/undo` A3 merges two saves of one note within 60 seconds into one "Edit Note" step.
- Owner decision 2026-11-24 (Mac Browse Note inspector; supersedes Option A's individual field disclosure and shared preview/fields scrolling): the main task is editing the visible note while seeing the desired card side. Individual fields never collapse from their label, content or surrounding whitespace; all nonempty fields and revealed empty fields stay visible even when the note type marks them collapsed. The separate aggregate Empty Fields disclosure and Add/Edit form behavior stay as before. A native Front | Back segmented picker near Preview selects the side directly and follows click-to-flip; saving the same card preserves the chosen side, and navigating to another card starts on Front. Audio remains manual and follows the shown side. The header takes two rows when its controls cannot fit. Below the header, a native vertical split places the card above an independently scrolling editor, with the horizontal divider immediately below the card. The card starts at 160 points, with a 96-point minimum; the editor has a 180-point minimum including its pinned formatting bar. Folding removes the card's space without replacing the editor. Native divider dragging supplies the resize cursor and accessibility; resizing keeps the caret in sight. Card-template styling remains untouched. Functional build, VM interaction/screenshot evidence and native design acceptance are pending integration verification.
- Built 2026-11-24 (U274; the implementer's reading, pending the coordinator): the Note | Info segmented control sits centred at the pane's top, above both tabs; the tab chosen is kept on the device. View › Card Info ⌥⌘I shows the pane with Info, and hides it when Info already shows. Info shows the card the Note tab's preview shows (Notes mode: the lowest ordinal, or the one "Card <i> of <k>" chose). No row and several rows show on both tabs as the Note tab shows them (V7), though V1 says the Info tab follows the current row: a reading for the coordinator. Info is one scroll view: C4's rows, label leading in the secondary style and the core's value trailing, each row one accessibility element ("Due, 2026-11-02"); Review History, newest first, in five columns (Date, Type in its state colour, Rating with Again in the Learn colour, Interval, Time; the date and time as a help tag), each entry one element; the Forgetting Curve in Swift Charts (past solid, future dashed, the desired retention dashed when C7.5 draws it) with the segmented range picker when more than one range is offered; Copy Debug Info, a bordered button whose title reads "Copied to clipboard" once copied. Verified in the test VM, light and dark: a reviewed card, a new card, several rows. Not built: the Ease and Stability history columns (the pane is too narrow for them at 320 points), the curve's tooltip, today marker and disclosure, placeholder rows while loading, and the local-midnight refresh (C10.2); the core's watch (U103) is not built, so any added review-log row reads the shown card again.
- Owner decision 2026-11-25 (Mac Browse inspector; supersedes the Note | Info control's place at the pane's top and the HTML control's place at the bar's trailing edge): at the top, Note | Info sat right above Preview's Front | Back picker, two like segmented controls that read as one group though one switches the pane and the other only flips the card. The pane now ends in one bottom bar under a separator, in every state (no row, several rows, Note, Info): the formatting controls at its leading edge (Bold, Italic, Underline, Cloze Deletion when it applies, Attach, HTML, in that order) while the Note tab shows a note it can edit, and the Note | Info segmented control at its trailing edge always. The bar is the pane's, no longer part of the editor under the split; the Preview header is the first thing in the Note tab. Chosen from an HTML example comparing this with keeping the control at the top and making Front | Back smaller. Functional build, VM interaction/screenshot evidence and native design acceptance are pending integration verification.

### N21.5 Several rows selected (7A)

- The inspector shows a title ("12 Cards Selected"), one secondary line (V7), then the commands as plain buttons in one column, each with its key at the trailing edge, Delete last in red. No toolbar change.
- The row context menu holds the same commands with their keys, grouped: [Change Deck…, Add Tags…, Remove Tags…] | [Flag ▸, Suspend, Unsuspend, Bury, Unbury] | [Set Due Date…, Forget…] | [Change Note Type…, Edit Note Type…] | [Find and Replace…, Find Duplicates…] | [Delete].
- Built 2026-11-23 (U273; the implementer's reading, pending the coordinator): the summary's commands are borderless plain buttons, full width, title leading and key trailing in the secondary colour, in the context menu's groups with a separator between groups; Flag is a menu row with a trailing chevron; a command V8.1 does not show for the selection is left out, a later unit's is in its place, disabled. The deck in the summary line is its full path joined with " › ".
- Built 2026-11-23 (U275; the implementer's reading, pending the coordinator): V8.1's "Shown when" leaves a command out of the context menu (Apple's rule for context menus) and disables it in the menu bar, whose items stay put. The context menu acts on the rows it was opened on (the selection, or the one row a secondary click outlines outside it), taken as index ranges of the list at its revision when the menu is drawn: a list that moves on while the menu is open makes the core refuse them (`browser/bulk-framework` K1), so no command acts on rows the person did not choose. A Delete after its question runs on the rows it asked about at that revision; when they moved meanwhile it deletes nothing and says so. Until the counts of the rows arrive from the core (a few milliseconds), both commands of a pair show and Unsuspend and Unbury read "Unsuspend Cards" and "Unbury Cards"; in Cards mode the rows on screen give the counts at once. Commands whose sheet or pane a later unit builds are in their places, disabled: Change Note Type… (U284); Change Deck…, Add Tags…, Remove Tags…, Set Due Date… and Forget… open their sheets since U276, Find and Replace… and Find Duplicates… since U277 (N21.6). The flags' items carry `flag.fill` in their colours, drawn as a non-template image so the menu keeps the colour (lint allowlist entry). A command the core refuses or that fails shows one alert, "Nothing Was Changed" ("Meguri couldn’t finish the command on these rows. Try again.", OK): V8 is silent on failures. In the standard column set the status line's set name stays plain text: V2.2 gives its menu no item there until U272's column chooser.

### N21.6 Sheets

- Change Deck…: a sheet with round 6's deck search and create row. Add Tags… and Remove Tags…: a sheet with a token field. Set Due Date…: a sheet with one text field (days, or a range "3-7"), its footer explaining the forms. Forget…: a sheet with the two options and "Review history is kept.".
- Find and Replace… (8A): a window sheet (Mac), form sheet (iPad, the Duo opened), full sheet (iPhone): the rows of V9 as a system `Form`, then the preview list, then a bottom line with "Undo restores every note.", Cancel and "Replace in <c> Notes" (the default action).
- Find Duplicates…: a small sheet with Field and Notes and Find.
- The Delete question: N20.7's forms (Mac alert; touch confirmation dialog).
- Built 2026-11-24 (U276; the implementer's reading, pending the coordinator): the five sheets share one frame, which Save Search… uses too. On the Mac the title leads in the headline style over the content, as round 6's New Deck form (N20.3), and Cancel and the default button sit at the bottom trailing edge in the system's places; no box around the content. The default button is never destructive (Forget too: Undo restores), Return runs it, Esc cancels. Change Deck…: the system search field (prompt "Search"), then a list of the normal decks as N20.1's two-line rows with a checkmark on the decks the target cards are in now, then the "Create deck named “…”" row while D18's read finds no deck with the typed text; ↑ and ↓ in the field move the highlight (with no text the selection's own deck when it has one, otherwise the first row), Return or a double click moves the cards, the button reads Move. Filtered decks are not listed (they are never targets, `browser/bulk-actions` B7.1), and the default deck only while a card is in it (`decks/deck-list` H6.1). Add Tags… and Remove Tags…: the system token field completed from the collection's tags (Remove: only the target notes' own tags, read in the core), a footnote on spaces and "::" (Remove: removing a tag also removes the tags inside it), buttons Add and Remove. Set Due Date…: one field "Due in Days" prefilled with `setDueBrowser` and selected, the day it gives under it ("Due in 3 to 7 days") or why it is refused (the core's parser, S2), the footer explaining the forms; Set. No interval choice: with FSRS on the `!` suffix changes nothing (S4.2). Forget…: the two toggles prefilled from the browser keys, "Review history is kept." as the footer; title "Forget Cards", button Forget. Each sheet acts on the rows it opened on, at that revision: a list that moved on is refused, nothing is written, and the sheet says so under its content; while the command runs a progress bar shows there and Cancel stops it until the commit. Save Search…'s Save is disabled while the name is empty or white space.
- Built 2026-11-24 (U277; the implementer's reading, pending the coordinator): Find and Replace… (⌥⌘F, also with no selection) and Find Duplicates… use the frame above. Find and Replace is a system `Form` in the columns style with V9's rows (focus starts in the first field; the fields offer F2's history as input suggestions; In groups the fields under each note type's name when there are several; Notes is a horizontal radio group whose second choice is enabled once the results are whole; the three options are checkboxes), then V9's count line and preview list (changes marked as V9 says, the last list kept while a newer preview counts), W2.1's error under the first field, the undo line, and Cancel with the counted default button, enabled only for the preview of the inputs shown. Find Duplicates is a `Form` with V10's two rows, its no-group line under the form, and Cancel and Find; its groups become one token whose kind the system token draws, the rest of V10's label after it, in Notes mode with the field's column first and sorted by it; a header click then sorts the rows shown and writes no column set. A disabled Find and Replace… or Find Duplicates… in the context menu or the inspector says why in its help.

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
- Built 2026-11-23 (U273; the implementer's reading, pending the coordinator): Note › Edit Note ⌘E in Browse is enabled with one row selected and its note open in the inspector: it shows the inspector and puts the keyboard focus in its first shown field. The Format menu, between Note and Card, holds Bold ⌘B, Italic ⌘I, Underline ⌘U | Superscript ⌘=, Subscript ⇧⌘= (`editor/formatting` F14) | Cloze Deletion ⇧⌘C, Cloze Deletion (Same Number) ⌥⇧⌘C; they act on the inspector's focused rich field and are disabled otherwise. Highlight, Text Colour, Lists and Remove Formatting are not in it yet: they come with the editor's formatting unit (U192). The inline formats are the field page's own transforms of F3 and F4 (wrap each selected inline run, merge with a neighbour of the same format, remove from the selection and keep around it); F4's extension over neighbours (step 6) is not built.
- Built 2026-11-23 (U275; the implementer's reading, pending the coordinator): Edit Note ⌘E moves from the Card menu (N20.8) to the Note menu, as this table places it; in review it is N20.8's, in Browse it waits for U273's inspector (disabled). Bury Card ⌘J and Suspend Card ⇧⌘J stay one item each: the review's card in review, Browse's selection in Browse (titled "Bury Cards" for several), so no key is on two enabled items. The Card menu reads: Show Answer | Again, Hard, Good, Easy | Change Deck…, Flag ▸ | Bury, Unbury, Suspend, Unsuspend | Set Due Date…, Forget… | Back to Decks; Show in Browse comes with U280. ⌥⌘A is a key equivalent of the window with no menu item (as Find…'s ⇧⌘O), and Select All is the system's (⌘A selects every row while the table has the focus). View › Card Info ⌥⌘I is in its place, disabled until U273 and U274. Delete Note ⌘⌫ is enabled only while the table has the focus and no text input does.

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
  Built 2026-11-24 (U283 follow-up): the window's width decides (the detail's own width changes when the sidebar hides); the sidebar comes back as it was once the window is 1,100 points or wider again or the Cards tab is left, unless the person opened it meanwhile; what the app remembers is the person's own choice.
- Nothing flashes (added 2026-11-22, after Antoine's report from the Mac app; decided by Claude (coordinator) under his delegation): choosing another note type keeps the shown one on screen until the new one's fields can draw, then the detail changes in one step. The Mac's title and its "n notes · c cards" subtitle change together once the new note type's counts are read (they are read ahead with the record), so the subtitle never empties. A wait longer than about half a second shows a progress indicator over the old content, never an empty page. After Save or Reload the version shown stays, read-only, until the note type is read again. The rows beside the selection, and on iPad the rows on screen, are read ahead with pure reads. The issues list says "No issues" only once its reads have answered.
- Changed 2026-11-22 (U281 iPad): on iPad the note type list is the section's root (large title, the system search field as its filter) and a note type opens as a navigation step with its own page (N22.7), instead of the list beside the detail. Accepted by Claude (coordinator) under Antoine's delegation, after the built app was compared in landscape and portrait.

### N22.2 Fields tab (2B)

A native `Table` (Name, Sort, Used On, Notes with a share indicator) with drag reordering, + and − under it, and an inspector beside it holding Name, Collapsed in Editor, Right-to-Left and Placeholder (`notetypes/workspace` W4). No Advanced group in this version. A deleted field is crossed out until Save.

Changed 2026-11-22 (Antoine's annotation on the Mac toolbar with the inspector open; decided by Claude (coordinator) under his delegation): the inspector's toggle is the last item of the window's toolbar, at its trailing end, as in Pages and Xcode. While the inspector shows, Revert and Save and then the toggle sit in the toolbar above the inspector; while it is hidden they trail the detail's toolbar, which then reaches the window's edge. Fields | Cards stays the detail's centred control. The order at the trailing end is the sync status (when it is there), Revert and Save, then the toggle.

### N22.3 Cards tab (4B, 5A)

Three columns: the card types list (with counts and an issue dot) over the Fields list; the code editor with Front | Back | Styling and the Insert menu in its bar and the issues list under it; the live preview with its note menu, Question | Answer, Light | Dark (sun and moon), and the Cloze Number menu for cloze types. No device or size picker.

Changed 2026-11-22 (Antoine's annotations on the captures of PR #114; decided by Claude (coordinator) under his delegation; `decisions.md`, rows `noteTypes` of 2026-11-22):

- Column hierarchy, as Mail, Notes and Xcode draw theirs: only the app's navigation sidebar uses sidebar material. On the Mac the note types list is a content list on the window's own background with the system selection, under its filter field; the card types column belongs to the editor's side of the window, on the same background as the editor's bar and issues, not a third sidebar. No custom greys or materials.
- The card types column is one list with two sections: the card types, with + and − under them (borderless, the list's own controls on its background), then Fields under a section header. No separate strips or bands between them.
- Changed 2026-11-24 (Antoine's decision in chat, after three mockups; `decisions.md`, row `noteTypes.templates` of 2026-11-24), replacing the line above where they differ: + sits at the trailing end of the “Card Types” bar, borderless, and its help names the card type it duplicates (`Duplicate “Standard”`), or gives the reason while nothing can be added. There is no −: it was disabled for every note type with one card type. Each card type's row has a More button (the system's borderless pull-down, `ellipsis`) between its name and its cards, with the context menu's commands (Rename, Duplicate, Move Up, Move Down, Delete; Restore for a crossed-out row). On the Mac it shows on the row under the pointer and on the selected row, and keeps its place while hidden so the cards do not move; on iPad every row shows it. Delete also stays ⌘⌫ and the context menu. Nothing sits between the last card type and the Fields header. VoiceOver gets the same commands as the row's actions; the More button itself is left out of the accessibility tree, since a button inside the combined row would become the row's press. The keys show beside the context menu's commands only: the More menus stay in the view tree, one a row, and keys on them would act on a row nobody chose. Delete, while one card type remains, is disabled with W5's reason as its help in both menus. `notetypes/workspace` W5 still reads "+ and − under it"; its text waits for a spec-writer session, and until then this line is the layout's authority.
- Each column of the Cards tab starts with a bar of one height (the layout with the lists shown; N22.7's hiding of the lists while a card type is open waits for the preview, U283): "Card Types" over the list, Front | Back | Styling and Insert over the editor; their hairlines make one line across the tab. On iPad a hairline also marks the navigation bar's edge across the full width (the Fields tab has the same), and both columns and their divider run to the bottom edge, under the home indicator; the lists are plain, edge to edge in their column, with standard separators and touch-sized rows (no inset rounded groups).
- Front | Back | Styling stays the system's segmented control, unstyled. On this macOS a segmented control in content draws its selected segment in the accent colour, with every AppKit segment style, while the toolbar draws its own grey selection (checked in the test VM on 2026-11-22); both are the system's, so neither is restyled to match the other. On iPad the Fields | Cards control has no toolbar glass of its own around it (one capsule, not a capsule inside another), which replaces N22.5's "own glass shapes" for that control; Save keeps its own glass and the job status keeps N22.5's places.
- The styling's colour swatches sit on their value's line, centred on its capitals and inside its ascender and descender, so at any text size they touch neither neighbouring line. Exception (functional): on the Mac every style of the system colour well draws larger than a line of code (checked in the test VM), so the swatch is a circle Meguri draws in the value's colour with a hairline, sized to the capitals, that opens the system colour panel; on iPad the system colour well stays and is scaled into the same size.
- An issue's fix button follows its message when both fit on one line; otherwise it goes on its own line below the message, so the message keeps the full width.
- Flip (A15) is one step of the Back's own undo ("Undo Flip") that puts back both texts; a front edited after the flip is kept, and only the Back is put back.
- Built 2026-11-23 (U283; the implementer's reading, pending the coordinator): the preview is the Cards tab's third column on the Mac, after the card types column and the editor, in the same split view, resizable (300 points at least, 380 ideal), with a bar of the other columns' height: the note menu ("<i> of <k>", Previous, Next, Fill Empty Fields; "No Notes" for a type without notes), the Cloze Number menu for cloze types, then Question | Answer and Light | Dark (sun and moon) at its trailing end. Its toggle, Show Preview | Hide Preview (`sidebar.trailing`), is the last toolbar item on the Cards tab, as the Fields tab's inspector toggle is on its tab, and is kept on the device. Narrower than the card types column at its narrowest, a 320-point editor and the 350-point preview (870 points of tab; changed 2026-11-24, after the captures: the preview's narrowest now holds its whole bar, the note menu, Question | Answer and Light | Dark, at the default window), the preview hides by itself and the toggle is disabled with the reason as help ("Make the window wider to show the preview."); wider again, the person's choice shows. Where the preview's column cannot hold its whole bar, the Cloze Number moves into the note menu, then the note menu shows only its symbol (its position as help), then Light | Dark moves into it, so the bar never touches the column's edges or clips. Under an error the last card stays behind a layer of the pane's own background; the page itself is never made translucent or untouchable, which on the Mac took the window toolbar's background away (the columns' separators then ran up through the toolbar). On iPad, with the preview shown, N22.7 applies: the lists step aside, the code and the preview take half each, and the card type is a menu at the leading end of the editor's bar; with it hidden, the lists and the editor show as before. The toggle is the last item of the note type page's toolbar there. The side follows Front | Back (Styling keeps the side shown) and Question | Answer changes it directly. Light | Dark starts as the app's appearance. The page is review's own page pair with the note type's CSS; each side is shown alone. It redraws about 300 ms after typing pauses; the card on the page stays until the next one is ready, and no spinner shows. A template error leaves the last card faded under the status line's words for that card type and side (the system's content-unavailable layout), never the error page; a crossed-out card type says it will be deleted. Card media need a tap (the preview's web configuration). A replay command plays only within 1.5 seconds of a click or tap on the card, so a card script that calls the replay bridge by itself plays nothing (changed 2026-11-24, U283 follow-up). Links in the card are inert in the preview: a click on one opens nothing (no navigation in the page, no new window, no browser), since the preview has no behaviour presenter; the page has no answer or write path. The note previewed is the type's newest note, or one a way in names (W14, wired by U287).

### N22.4 Styling (6B)

Only while Styling is selected, the localized "Shared by All Card Types" guidance appears in the part picker's native help and accessibility hint on the Mac, with no inline caption in its bar. On iPad, the existing secondary label stays visible in the non-macOS picker bar while Styling is selected; the picker help and hint apply only on the Mac. Above the CSS is one Card Text row: Font, Size, Alignment (a three-segment control), Text Colour and Background (colour controls). The row is absent when the CSS has no single plain `.card` rule (`notetypes/workspace` W10).
- Built 2026-11-24 (U283; the implementer's reading, pending the coordinator): the row sits under the editor's bar while Styling shows, with a hairline under it: "Card Text" in secondary text, Font (a pull-down: System, then the system's font families), Size (a number field with a stepper; a step from no size starts at 16), Alignment (a three-segment control of the system's text alignment symbols), Text Colour and Background (system colour controls, small; a colour is written once it rests for a quarter second, so a drag in the colour panel is one edit). Where the editor column is narrow the title goes first, then the font menu narrows, then the controls take two lines. A change goes through the styling's text view as one edit of its own undo, the value written shown with the system's find indicator on the Mac. The core's pure reads write the CSS (U268): only the one value changes.

### N22.5 Save (3C)

One Save in the toolbar. When D3 asks, a sheet titled `Save Changes to “<name>”?` lists the changes (losses first, in red, each with its count), then the sync paragraph, then Cancel and the confirm button (Save and Upload with Meguri Sync; Save otherwise; destructive when content is lost). Progress while it saves shows in the toolbar ("Updating 212 of 412 notes").

Changed 2026-11-22 (Antoine's feedback on the captures of PR #112: on Mac "I can't help but think those don't look really good", on iPad "This section looks kind of strange as well"; decided by Claude (coordinator) under his delegation; `decisions.md`, row `noteTypes` of 2026-11-22):

- While a save job runs (once it has run for half a second, D5 step 5), the toolbar shows one status item in place of Revert and Save: a small system progress indicator and the stage text ("Syncing…", "Updating 212 of 412 notes", "Uploading…"; "Saving…" until the first count arrives), with no glass of its own. Its width is the widest text the job can show, with monospaced digits, so the toolbar does not move as the text changes. Revert and Save are not shown until the job ends; the note type stays locked (W3).
- No disabled prominent button: Revert and Save are one toolbar group in the standard style, and Save is not prominent, since it is disabled until there is a draft.
- iPad: Fields | Cards, the sync status (N23.6) and Save are separate items with their own glass shapes, in that order at the trailing end (on the Mac the order is the sync status, Revert and Save, then the inspector's toggle); the list of note types has the sync status in its own trailing place; the job's status sits in the bottom toolbar's status place, as Mail's does, so nothing wide sits in the navigation bar beside the tabs at any width. At compact width (the iPhone and the Duo folded, where the tab bar takes the bottom bar's middle), the spinner takes Save's place and the stage text takes the subtitle's place under the note type's name.
- This replaces the earlier toolbar, where the progress, Revert and a tinted Save were three separate shapes and, on iPad, the progress overlapped the tabs.

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

Decided by Antoine in chat on 2026-11-20 (`decisions.md`, row `sync.statusPopover`; round 10 answer 6A). Changed 2026-11-21 (round 10). In force: approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/round10-sync-panel-20261121.md`). The text is `sync/status-strings` T20; the status itself is T19.4 and T19.5, unchanged. Where the earlier text that N23.5 lists differs, this section wins. The sidebar and its frame are not part of this section. The lines marked "Changed 2026-11-21 (resume fix)" are in force: decided by Claude (coordinator) under Antoine's delegation of 2026-11-21 and his permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/sync-media-resume-20261121.md`).

### N23.1 The panel

- The sync status button opens one small panel: a popover on Mac and on iPad at regular width, `ContentWidth.primaryAction` (320 pt) wide and as tall as its content; at compact width (iPhone, the Duo folded or held tall) a sheet with the same content, the medium and large detents and the drag indicator, which scrolls at the largest text sizes. Both services, every state.
- Changed 2026-11-22 (Antoine on his iPhone Duo: opened, "Too much space below the downloading media"; folded, "same issue here"; decided by Claude (coordinator) under his delegation): the sheet is as tall as the panel. It has one detent, measured from the panel's own height, which grows with the text size and stops at the screen's height, where the panel scrolls; there is no larger detent that would only show empty space, and so no drag indicator. The sheet opens at that height: the panel is measured before the sheet shows, not first shown at a default height. Where the popover itself becomes a sheet (an iPhone at regular width, such as the Duo opened), that sheet fits the panel the same way. This replaces the medium and large detents and the drag indicator above and in N23.5.
- Top to bottom, with no inner box, form group, divider or section header:
  1. One status line: the state's symbol (N23.2) leading, the status title beside it.
  2. Under the title, in secondary text, the status detail: one sentence, or none when the status has none. A detail that repeats the title is not shown, here or in the Sync section.
  3. While the work runs (below), one linear progress bar under the text, and under it, while media transfers and a count is known, "N media files left" in footnote secondary text (T20). Changed 2026-11-21 (resume fix): for iCloud only; Meguri Sync's sentence carries its files left (T19.5).
  4. One row of buttons at the trailing edge: Sync Settings, then the state's action (N23.3). When the two do not fit on one line (the largest text sizes), they stack, trailing, the action first.
- The panel shows what the Sync section's status row shows: its symbol, title, detail and action come from the same main status (T19.4, T19.5), so the two never disagree. It leaves out the rest of that row: T4's lines, T5's other media lines, T7's step and result lines, T19.3's setup-done lines, an iCloud connection failure's line (`settings.sync.connectionError`) and Meguri Sync's media facts (T18.6) stay in the Sync section, which now shows those media facts under its status row. With issues, one footnote secondary line under the detail gives their count, as the Sync section's issues row reads (`sync.issues.row`, or Meguri Sync's `sync.meguri.media.notTransferred`), as plain text; the list opens from the Sync section (N15.4). This line goes beyond 6A's "one sentence" on purpose: without it the panel would say less than the Sync section about the same status.
- The bar: for Meguri Sync it is the Sync section's status-row indicator (changed 2026-11-21 (resume fix): before, the Account row's), with the same value, whenever it shows one (`sync/meguri-provider#MP6.5`, "One progress indicator": during the media step files done over done plus left, indeterminate until the first count); otherwise it is indeterminate while Meguri Sync signs in or a round exchanges. For iCloud it is indeterminate while the state shown (T19.4 rule 2: `stateWithoutAttention` under `needsAttention`) is `syncingCollection` or `syncingMedia`. While Meguri Sync prepares a sign-in, the Account row shows its labelled spinner (`sync.meguri.signIn.preparing`); the status is still the sign-in row, so the panel shows that row and no bar until signing in starts. A status that needs you, and a blocked or stopped status, has no bar (MP6.5). iCloud's `syncingMedia` has no detail (T3); its files line is under the bar.
- Only the toolbar button's symbol turns, exactly while the bar shows (N23.2). The panel's and the Sync section's symbols stand still.
- The panel follows the status while it is open: a change of state changes its text, bar and buttons in place. The person closes it (Esc, a click outside, the sheet's drag); it also closes when one of its buttons opens the Sync section, when a window sheet opens (for example `sync.keepCollection` by itself), and when the size class changes.
- Mac: Return runs the default button, when there is one; Esc only closes the panel. With keyboard navigation on, the buttons are in the panel's Tab order.
- Accessibility: the panel is `sync.status.popover`; the status line, its detail and the issues line are one element, `sync.status.panel.status`, read as the title, the detail, then the issues line; the bar is `sync.status.panel.progress`, labelled with the title, with the system's value; the files line is `sync.status.panel.filesLeft`; the buttons are `sync.status.settings`, `sync.status.syncNow` and `sync.status.action`. Choose… keeps its accessibility label `sync.decision.action.label`, as in the Sync section.

### N23.2 One symbol family

The toolbar button, the panel's status line and the Sync section's status row use the same symbol, one of five states, for both services. Each is one system SF Symbol, drawn by the system at the size and weight of its place; nothing is drawn over or inside a symbol. Only needs you has a colour, the system orange on the whole symbol; in the toolbar Sync Off is in the secondary colour. This extends the decision's "in the toolbar" to the panel and the Sync section, so the three never disagree.

Apple reserves every `icloud` symbol (`icloud`, `checkmark.icloud`, `arrow.trianglehead.2.clockwise.rotate.90.icloud`, `exclamationmark.icloud`, `icloud.slash`, their fills and the others) for Apple's iCloud service: the SF Symbols metadata says they "may only be used to refer to Apple's iCloud service" and "may not be modified". SF Symbols has no unrestricted cloud with a check, arrows, an alert or a slash. So iCloud uses the `icloud` symbols, and Meguri Sync and Sync Off use unrestricted symbols for the same five states (N23.4).

Changed 2026-11-22 (Antoine's feedback on the Mac toolbar: the alert symbol inside the cloud "looks strange. Not aligned", and for syncing "the loop inside the cloud is not very well aligned"; he offered to "fix or go with another icon"; decided by Claude (coordinator) under his delegation; `decisions.md`, row `sync.status` of 2026-11-22). Before, Meguri Sync and Off drew the `cloud` symbol with a smaller symbol set within it by our own view; that placement sat low and off centre at the toolbar's size.

| State | Means | iCloud | Meguri Sync and Off | iCloud states (`sync/policy#P14.2`, T19.4) | Meguri Sync states (T19.5) |
|---|---|---|---|---|---|
| done | up to date | `checkmark.icloud` | `checkmark.circle` | `upToDate` | round idle, Sync Automatically on |
| moving | something moves | `arrow.trianglehead.2.clockwise.rotate.90.icloud` | `arrow.trianglehead.2.clockwise.rotate.90` | the state shown (T19.4 rule 2) is `syncingCollection` or `syncingMedia` | signing in, preparing, transferring the collection or media, round exchanging |
| waiting | waiting, or not sent yet | `icloud` | `cloud` | `changesPending`, `offline`, `paused{automaticOff}`, `iCloudUnavailable{temporary}`, a waiting row of T19.4, no status yet | setup required before preparation, the keep-collection sheet open, round pending, waiting or cancelled, Sync Automatically off, a waiting row, media blocked, any other title |
| needs you | needs you | `exclamationmark.icloud` | `exclamationmark.arrow.trianglehead.2.clockwise.rotate.90` | every needs you row (T19.4) | every needs you row (T19.5) |
| off | sync off | — | `cloud`, in the secondary colour in the toolbar | no service selected: the Service control reads Off, and an iCloud projection then reports `localOnly` (with iCloud selected, `localOnly` is T19.4 row 16, needs you) | |

- The moving symbol turns with the system rotate symbol effect (`symbolEffect(.rotate)`), exactly while the panel's bar shows; with Reduce Motion on it does not turn.
- Off and waiting are both `cloud` for Meguri Sync; in the toolbar off is in the secondary colour, and the accessibility value and help tag name the state (below).
- An issues row keeps the symbol of the state underneath (N15.4).
- N16.2's rule for showing the button during review is unchanged and separate: Meguri Sync signed in with setup not continued shows the button with the waiting symbol, since T19.5 gives those rows no kind, or waiting. Only needs you rows use the needs you symbol.
- The button's accessibility label is the service's name (`sync.button.label`, `sync.meguri.button.label`), and with no service `sync.button.label.off` (T20.2); its value is the status title without its "…"; on Mac its help tag is the title and the detail. The state is never given by the symbol alone.

### N23.3 The action

The first row that applies decides, in this order.

| Status | Buttons, trailing |
|---|---|
| Sync is off (no service, and the provider does not need the person) | Sync Settings, as the default button |
| Needs you, with a built action (T19.4, T19.5 Action column) | Sync Settings, then the action, as the default button unless the "No default button" bullet below says otherwise |
| Needs you, with no built action (iCloud's `noAccount` and `restricted` states, Update Meguri (T19.4 row 4), Abandon Change… (row 25, not built)) | Sync Settings, with no default button: Return does nothing |
| Not needs you, with the row's own action (Cancel Setup while setup prepares; Try Again for an acceptance that is not known, T19.1; Try Again for a paused setup media step, changed 2026-11-21 (resume fix)) | Sync Settings, then that action; Cancel Setup is never the default |
| Meguri Sync with no round reported yet, or with round media blocked | Sync Settings, then Sync Now, disabled, as the default button, with no help tag; File › Sync Now and Y still open the Sync section (T18.7) |
| Any other status with a service | Sync Settings, then Sync Now as the default button |

- Only the status action counts (T19.4 rule 1, T19.5's Action column). The actions of an issues row (Retry, Check Database…) stay in its list.
- A built action shows also while it cannot run, disabled, with no reason line (Try Again is enabled as Sync Now is, T9.2). An action whose conditions do not apply (Cancel Setup while nothing prepares, Cancel Replacement… with no operation, Meguri Sync's Choose… while its work runs, Copy Debug Info with no diagnostics) is not shown. For a needs you row the panel is then the third row's (Sync Settings, no default button); otherwise Sync Now keeps its place.
- No default button: an action that runs a collection replacement at once (Finish Setup, Resume of a waiting replacement), Cancel Setup, and Copy Debug Info (changed 2026-11-22, same decision as N23.1's fitted sheet: a diagnostic does not resolve the state, so it is never the prominent button; this covers the damaged-collection row, whose built action is Copy Debug Info). Return then does nothing. Actions that open a confirmation first (Cancel Replacement…, Set Up Again…, Choose…) can be the default: their confirmation is the step that changes anything, and its destructive button is never the default (N11.8).
- Sync Now stays in its place while the status moves, disabled, even where `sync/policy#P12.1` would let a press join the running exchange, so the buttons never jump. Outside the panel nothing changes: File › Sync Now, Y and the Sync section keep their availability and their reason (T19.7); the panel shows no reason line, and the Mac help tag of its Sync Now gives the reason where T19.7 has one. A running Sync Now has no Cancel in any status surface (the panel, the Sync section, T19.6).
- When the state needs you, its action takes Sync Now's place: a disabled Sync Now with a reason would only repeat the sentence.
- An action that runs at once runs from the panel, which stays open and updates (Sync Now, Try Again, Resume, Finish Setup, Copy Debug Info). An action with a confirmation (Cancel Replacement…, Change the service recovery's action): on iPhone and iPad its confirmation shows over the panel, as the Sync section's row does; on Mac the panel closes and the action opens the Sync section, whose row asks, since a transient popover can close when its alert takes the keys. Meguri Sync's sign-in action is Sign In… and opens the Sync section at the sign-in fields; an action that opens a sheet of the Sync section (Meguri Sync's Choose…, a setup started again) opens the Sync section, which presents it (N15.3). iCloud's Choose…, Set Up Again… and Resume Syncing open their sheets directly, as built (for Resume Syncing see N23.5). On iPhone the panel's sheet closes before the Sync section or another sheet opens, so no sheet sits on a sheet.
- Copy Debug Info is in the panel only where it is the status action: iCloud's `setupNeeded` state and Meguri Sync's blocked provider, the two states of the decision; Meguri Sync's collection it cannot use (`localUnusable`, added 2026-11-20); and iCloud's damaged-collection row (T19.4 row 1), where, until the damaged-collection screen is built, the built action is Copy Debug Info in place of Show Repair Options. Everywhere else Copy Debug Info is in Troubleshooting (N15.5 row 6). While it copies it is disabled, as in the Sync section.
- Sync Settings opens the Sync section: the Settings window on Mac, the Sync section of Settings on iPad and iPhone, and during a review on iPhone and iPad a sheet over the review (N15.4).

### N23.4 Meguri Sync's symbols

Changed 2026-11-22 (N23.2). This section was the exception for Meguri Sync's drawn cloud shapes (the view `CloudShapeSymbol`, a `cloud` with a check, arrows or an alert set within it, or a drawn slash). That exception is withdrawn and the view removed: no symbol is composed or drawn.

- Why not the `icloud` symbols: Apple restricts every one of them to Apple's iCloud service (N23.2), and Meguri Sync is not iCloud.
- Why not a cloud with a badge: SF Symbols has no unrestricted one, and our own placement of a badge inside `cloud` did not match the system's alignment at the toolbar's size (Antoine, 2026-11-22). A custom symbol made in the SF Symbols app could restore a cloud family for Meguri Sync later; it needs Antoine's agreement and the app, which this project cannot run unattended.
- What is used: the unrestricted system symbols of N23.2's table, drawn as they are. `exclamationmark.arrow.trianglehead.2.clockwise.rotate.90` is the system's own symbol for a sync that needs attention.
- Verification: the built app in the test VM, each state for both services, light and dark, on Mac, iPad and iPhone (the pull request's captures).

### N23.5 What this replaces

- N11.8, first bullet: "A distinct SF Symbol per `sync/policy` P14.2 state" becomes N23.2's five shapes (colour is still never the only signal, R104), and its "sheet with medium and large detents" stays, with the drag indicator. The `sync.status` screen contract's popover description and symbol lists for both services are replaced by N23 (that contract has a new version for N23).
- N15.4: "in the Sync section's status row and in the status popover" stays for the needs you sentence and its button; in the panel the issues row is a plain count line, and its list opens from the Sync section.
- `sync/status-strings`: T2's popover row; T3's `sync.status.keptLocally` "in the popover only" (not shown since T19); T6's reason line under the popover's Sync Now and its "beside the disabled Sync Now" for decisionRequired; T7's steps and results "in the popover" (the end of a Sync Now the person started is still announced, T1); T8's throttling line "only in the popover" (it shows in the Sync section's status row); T9.2's reason line under Try Again in the popover; T18.3's service label in the shared popover (the panel names no service; the button's label does); T18.7's three popover buttons (`sync.meguri.setup.continue`, `sync.meguri.now.chooseCollection`, `sync.meguri.media.showAffected`), which stay File › Sync Now's routes; T19.4 row 5's "no sheet follows" for Resume Syncing: until V27.6's direct resume is built, Resume Syncing opens the connect flow's continue page, as built, in the panel as in the Sync section; T19.3's setup-done lines "and the popover"; T19.4 rule 3's issues row "which opens the list", in the popover; T19.7's popover Sync Now with its reason.
- Plan text, for the popover only: R103 (offline and pending changes now share the plain cloud; the title in the panel, the help tag and VoiceOver tell them apart), R105 and R1560 (collection and media progress, the last exchange, pending work and the latest error: the panel shows the status, its bar and its sentence; the rest is in the Sync section).
- The popover's inner form group, its divider, its centred Sync Now with a reason line and its centred Sync Settings.

### N23.6 The button syncs, in its neighbours' group (2026-11-22)

From Antoine on his folded iPhone Duo: "It would be a bit better if this button was moved to the right sidebar, so to the vertical bar, because this way it would be available everywhere"; "why was it blue here? Once it's finished, it should go back to being a normal color"; "This way nothing opens"; and, on the Mac's Note Types screen, "The sync button should also most likely be to the right." Decided by Claude (coordinator) under his delegation (`decisions.md`, row `sync.status` of 2026-11-22, "The sync button syncs"). The text is `sync/status-strings` T22. Where N23.1 to N23.3, N11.8 or N14.2 differ, this section wins.

- **Placement, iPhone and the Duo folded or held tall (the tab layout's Decks stack).** The button is the first item of the trailing group that holds Search and Add, on the deck list and on every deck page, nested decks included: one group, which the system draws as one capsule (on the folded Duo the vertical capsule on the trailing edge). It is declared inside that group by each screen of the stack (the shell hands it down as `syncStatusItem`), never as a toolbar item of its own.
- **Placement, the Duo opened and the iPad.** On the opened Duo the list column carries it first in its trailing group (the detail beside it does not repeat it); on the iPad, Today and a deck page carry it first in their own trailing group in the same way. On the iPad, Browse and Note Types (and Statistics when it has a toolbar) get it as a trailing item of their own toolbar, on their root and on a page pushed there (a note type's); Settings does not, since it holds the Sync section.
- **Placement, the Mac.** A trailing item of the main window's toolbar on every screen, in the same place: first in the screen's trailing group, never between the title and a principal control. The shell hands the one button down (`syncStatusItem`) and each screen places it in its own trailing group: Today and a deck page before Add, Browse after its Cards / Notes control; Note Types puts it before Revert and Save (`[sync] [Revert] [Save] [inspector]`). An item declared on the split view itself comes before a screen's own items, so on Browse and Note Types it showed right after the title, before their principal control (Antoine's capture); that was the cause. Until Note Types' toolbar reads `syncStatusItem`, the shell still declares it there, in its old place.
- **Appearance.** A plain toolbar symbol of N23.2's table, drawn by the system at its neighbours' size and weight, in the default colour when up to date or waiting, in the secondary colour when off, in orange when it needs you. Never a filled, prominent or accent-tinted button; no text beside it (T2's media indicator is withdrawn).
- **A tap** (T22.1): syncs at once in the ordinary states; nothing opens. The symbol turns while the sync runs (N23.2's rotate effect, still with Reduce Motion), then, when it ends up to date, the check shows in the accent colour for about a second and returns to the default colour; when it ends in another state the button shows that state. A tap while the status moves does nothing. When the state needs the person, sync is off or not set up, the state has its own action, or Sync Now cannot run, a tap opens the panel (N23.1), as before.
- **A long press** on touch, and a click and hold on the Mac, show a menu (a secondary click on a Mac toolbar opens the system's toolbar menu, as the capture showed): the status line (title, then the sentence), Sync Now (where the panel shows it, enabled as the panel's) and Sync Settings. Everything the panel offered in the ordinary states is reachable without it. The Mac help tag is still the title and the sentence. File › Sync Now (⇧⌘Y) and Y are unchanged.
- **Accessibility.** Label "Sync" for every service, value the status title, hint "Syncs now." or "Shows the sync status." by what a tap does, none while the status moves. The end of a sync started from the button is announced (T1, T22.1).
- **Why it was a filled blue circle, and why it moved.** The shell declared the button as a toolbar item of its own (`ToolbarItem(placement: .primaryAction)` on the Decks stack's root only, so deck pages had none), apart from Home's Search and Add items, and wrapped it in a custom view: an `HStack` with a slot for iCloud's media text and, in its background, the hidden full-width panel that measured the sheet. In the up-to-date state its symbol is a check; drawn alone at the trailing edge, the system treated it as a confirming (Done) action and drew it as a prominent accent-filled circle outside the group. In the needs-you state (orange) the same item was drawn plain inside the group (PR #122's capture), and a lone item in the syncing state was a plain circle, so only the check state was singled out. When the large title scrolled away on the folded Duo, the system kept the trailing group in its vertical capsule and moved the separate item to a bottom bar beside the tab capsule (Antoine's second capture). The empty capsule he saw at the bottom during a long media download was most likely the same moved item in the syncing state, hosted there as the custom view, with its turning symbol not drawn; it went away when the download ended. Not reproduced: the VM's Duo could not be folded for this change. The fix removes every part of this: one group declared by each screen, no item of its own, no custom view or hidden measure in the toolbar (the measure is behind the shell's root), no text beside the symbol.
- **Verification.** The built app in the test VM over a generated collection with `-MeguriSyncRender <state>[+justFinished]`: the folded Duo (deck list at the top and scrolled to the bottom, a deck page), the iPhone, the iPad and the Mac (Today, Browse, Note Types with its inspector shown and hidden), light and dark, in the states up to date, syncing, just finished, needs you and not set up. `SyncButtonTests` and `SyncButtonPlacementTests` (T22.3).
- Replaces: N23.1's "The sync status button opens one small panel" for the ordinary states; N11.8's and N14.2's toolbar item of its own; the `sync.status` contract's "A click or tap opens the popover" and its media indicator.

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
- A click (a tap) opens the deck page, unchanged. A double-click (the Mac only: on the iPad a pointer's double-click cannot be told from a finger's double tap) or, while the deck list has focus with the row selected, Return start the deck's review, and so does S on the iPad; as `decks/deck-list` H3 and H9.2 say (built now: H12). On the Mac sidebar S is not built: the system list's type-select keeps the letter keys, a platform default (changed 2026-11-21). Exception (round 10, Mac): Return is taken by a key monitor of the main window, not by the list. Need: a row's click leaves keyboard focus with the row's own button rather than the list, so neither the list nor its primary action (`contextMenu(forSelectionType:primaryAction:)`, tried first) ever sees Return. It acts only on Return or Enter without Command, Option, Control or Shift (Caps Lock does not count), not on a repeat, in the main window with no sheet, and while no text input, sheet of the app's own or review has the keys (`SidebarReturnKey`). Verification: `SidebarReturnKeyTests` for each guard; `MacDeckListKeysTests` in the test VM for Return on a deck row and on Today, the double-click, and S in the search field. The first click of a double-click opens the page, as a click does; the second starts the review. A deck with nothing to study shows what H3 says (its finished state, or H5.4's empty view). An empty filtered deck offers Rebuild (`decks/filtered-study#F6`).

### N24.5 iPad and the Duo opened (5A)

- The same summary as the Mac, in touch sizes, at the deck page's inset (N14.3). The Duo held wide keeps the list beside Today with its Today row (N14.10), whose number is the cards waiting.

### N24.6 The heatmap's look (4B)

- **Levels**: `stats/activity-heatmap` AH7, rewritten 2026-11-21, gives each of the four tints to roughly one fourth of the person's studied days in view, with the top day always in the top tint. Because only the days in view count, a narrower or wider grid may shade a day differently; accepted.
- **Size.** Changed 2026-11-21 (Antoine, chat; `decisions.md`, row `stats.overview`), with the Mac Home sizing superseded by the owner on 2026-11-24. Each day is a rounded square:
  - Mac Home: every square is always 10 × 10 pt with a 2 pt gap and a 2 pt corner radius. Seven rows are always 82 pt high. More width reveals older week columns past 53; squares never grow. The newest week stays at the trailing edge.
  - Touch (iPhone, iPad, the Duo): a 16 pt square, a 3 pt gap and a corner radius of a fifth of the side, round 10's iPhone sketch (10, 2 and 2 px on a phone drawn about 0.63 of its real width) in points. About 17 weeks on an iPhone; on an iPad, as many as its column holds (all 53 on a 13-inch iPad's Today in portrait).
  - Select weeks = ⌊(W + gap) ÷ (square + gap)⌋, at least one once a positive finite row content width W is known; touch is capped at 53. A width below one square keeps the fixed-size current week. Touch keeps its natural width, leading-aligned under Activity. Mac Home uses its natural width, trailing-aligned, leaving less than one pitch unused at normal widths. Seven rows are reserved before data arrives. Width changes update layout only (N14.11's performance rules).
  - The month labels run over the grid's width, not the row's; the summary line follows the grid (the line stays leading-aligned under it and wraps at the row's width). On touch, today's outline also follows the grid; Mac Home has no today outline.
  - Dynamic Type: the squares are not text and do not scale with text size; the month labels (caption) and the summary line (footnote) scale, and the labels' row grows with them.
  - Replaces N14.11's stretched squares and minimum column pitch (20 pt on touch, 13 pt on the Mac) and the 2026-11-19 `stats.overview` decision's "It fills the row width with equal margins on every device".
- **Colours.** Changed 2026-11-21 (Antoine, chat: "change the color of the heatmap to using green tones"; `decisions.md`, row `stats.overview`). Green is a decided colour, not the accent: the heatmap stays green whatever accent the person picks. A day with no reviews keeps the system secondary fill. The four steps are the system green (SwiftUI's `Color.green`, which adapts to light, dark and Increase Contrast). No custom colour values. `ActivityGridTests` fixes the level-to-opacity tables:
  - Standard appearance, light and dark: 0.3, 0.5, 0.75 and 1. One table serves both: green is much lighter than the blue accent on a dark background, so the accent's stronger dark steps (0.5, 0.68, 0.84) left level 1 far from an empty day and levels 2 to 4 close together. These steps keep each level at least about 14 CIELAB ΔE from the next and at least about 20 from an empty day (system green and secondary fill over the row background, computed for the pull request).
  - Increase Contrast: light 0.8, 0.87, 0.94 and 1; dark 0.55, 0.7, 0.85 and 1; drawn over the empty day's fill. Every level reaches at least 3:1 against the surface it is drawn on (the rubric's `color-contrast` non-text rule), and each level's ratio is at least 0.3 above the one below. `ActivityGridContrastTests` checks this with the system green as SwiftUI resolves it with Increase Contrast, over a white surface in light and the iPhone's row and the Mac's window backgrounds in dark, leaving out the fill under the level (which only raises the ratios). In light the scale is narrow, between 3:1 and the green's own ratio (about 4.4:1), so its steps are closer together than in the standard appearance.
  - Exception (standard appearance), with its need: the levels are a graded scale whose lowest step is light on purpose, so steps sit below 3:1 against the surface: in light all four (the system green itself is about 2.2:1 on white; level 1 about 1.3:1), in dark level 1 (about 1.9:1) and level 2 on the lighter surfaces; all four above 3:1 could not also be told apart. Each level stays distinguishable from its neighbours and from an empty day (above). A day's count is not carried by colour alone on the Mac, where each square's help tag gives its date and count; the grid's one accessibility element gives the total of the days shown (the summary line, also printed under the grid). On touch, per N14.11, a day has no tap, callout or accessibility value of its own, so a single day's count is shown only by its shade; a per-day channel on touch is not decided (open). Level 1 stays at 0.3 in light: in the test VM's renders it reads clearly apart from an empty day.
  - Verification: the built app in the test VM in light and dark, in the pull request; Increase Contrast by `ActivityGridContrastTests` (no VM route sets it).
  - Replaces: the accent colour at 0.35, 0.55, 0.8 and 1 (0.4, 0.6, 0.8 and 1 with Increase Contrast; 0.5, 0.68, 0.84 and 1 in dark), the Mac's Graphite exception (the system blue), and round 10's custom-colour exception for the accent's steps.
- **Month labels** (AH9): above the grid, in caption secondary text, inside the grid's one accessibility element (not read on their own). The grid's height includes the labels' row from the first frame (N14.11's performance rules).
- **Today.** Changed 2026-11-24 (owner follow-up): Mac Home gives today's square the same style as every other day, determined only by its review count, with no outline. Touch keeps its 1 pt outline in the primary label colour. Statistics is a separate layout and keeps N25.8's today and pinned-day outlines. The Mac's hover help and the grid's accessibility are unchanged.
- The Mac's day help tag and the summary line are unchanged. Today's square counts AH2's rows, Studied Today counts T12's (N6's): on a day with Set Due Date or rescheduling rows they can differ; accepted.

### N24.7 What this replaces

- N14.1: its third bullet (the decks with cards waiting, each with Study), "Only the previous session keeps a section label", the nothing-waiting order ("The error line and the previous session follow") and "iPad's Today stays a plain list", for the sidebar layouts.
- N14.6: "The count is the To Review badge, hidden at zero (N11.5)", and N11.5's native badge with the To Review count.
- N14.11: the Colour bullet, "there are no month labels" (opening paragraph, Text, Width and Not in this build) and its Today placement ("after the decks with cards waiting"). Changed 2026-11-21: also its Width bullet's stretched squares and minimum column pitch (N24.6's Size).
- N16.5: "counts different cards answered in the session" becomes "in the walk" for a walk.
- `stats/activity-heatmap` AH7's earlier rule.
- Screen contracts: `home.deckList` has a new version (its badge and Today text); `home.today` is added to `screen-inventory.yaml` with the code.

## N25. Round 11: the Statistics page (2026-11-22)

Decided by Antoine in chat on 2026-11-22 (`decisions.md`, rows `stats.overview` of 2026-11-22; round 11 answers 1A 2A 3A 4A 5A 6A 7A 8A 9A, with All as the first period and the Forgotten chart in orange instead of red). Changed the same day by his loading decision (N25.9). In force: approved 2026-11-22 (R1357) with `stats/statistics-page` by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/statistics-round11-20261122.md`); Antoine approved the design in chat on 2026-11-22. Behaviour is `stats/statistics-page` and `stats/history-reads`; this section is the layout. Where earlier text that N25.13 lists differs, this section wins. Sketches: the round 11 design page (`build/library/round11/`, not committed; HTML is not native approval).

### N25.1 Where Statistics lives (8A)

- Mac and iPad: a sidebar row "Statistics" with `chart.bar.xaxis`, second under Today: Today, Statistics, Browse, Note Types, then the decks. One window: sidebar and the page; no separate window.
- View › Statistics ⇧⌘S (after Today's place, before Browse ⇧⌘B and Note Types ⇧⌘T). ⇧⌘S is free: Meguri has no documents.
- iPhone and the Duo folded or held tall: a tab "Statistics" between Decks and Browse (four tabs: Decks, Statistics, Browse, Settings); on the Duo opened, the side tab bar's second item.
- Deck Statistics (`chart.bar.xaxis`) in the deck menu (N20.2: group 1, after Browse Cards), the menu bar's Deck menu (after Deck Options…, no key) and the touch More menu; for normal and filtered decks. A deck page's Activity header is a button to the same place. T on Today and a deck page (Mac, iPad with a keyboard).
- The section stays out of `availableSections` until its page is built (`stats/statistics-page` S13). Changed 2026-11-23 (PR #161; the coordinator's decision under Antoine's delegation, `decisions.md`): it is in the product on the Mac and the iPad at regular width, where its page is built, as Today, Statistics, then Browse and Note Types (Mac) or Note Types and Settings (iPad); the iPhone, the Duo and the compact layouts kept it out until U304, which shows it there too (PR #162). `-MeguriStatsRollout off` hides it for tests.

### N25.2 The toolbar and its two controls (3A)

- Mac: the window toolbar, leading to trailing: the title "Statistics" with the subtitle "1,370 cards in 4 decks" (and the status of N25.9), the deck pop-up button, the period segmented control (1M 3M 6M 1Y All), then the sync button and the Add menu as on every page. Both controls are standard toolbar items and stay put while the page scrolls.
- iPad at regular width: the same in the navigation bar, as two trailing items kept apart (not one glass group), the pop-up's label carrying its up-and-down chevrons (U297, 2026-11-23).
- iPhone, the Duo folded, a narrow iPad window: the inline title "Statistics" with the deck pop-up beside it (a menu button with the deck's name and a chevron) and the subtitle under it; the period segmented control pinned under the navigation bar while the page scrolls; the Jump menu (`list.bullet`) as a trailing toolbar button. Changed 2026-11-24 (decided by Claude (coordinator) under Antoine's delegation; the Statistics follow-up PR #192): the narrow page's subtitle is "<n> cards" with its status ("4,210 cards · Updating…"), without the deck count, which made it truncate ("· Update…").
- While the controls are disabled (no collection, no cards), the Mac's period control is dimmed with the deck pop-up: an exception, since the window toolbar draws a disabled segmented control in full colour (seen in the test VM, 2026-11-23).
- The deck's name is visible at every scroll position on every device. A long name truncates in the middle, with the full name as help text (Mac).
- Kept state: `stats/statistics-page` S3.3 (per window and device; first time All Decks and All).

### N25.3 The Mac and iPad page (1A, 2A)

- One scroll view. The strip (five figures, no box: value, unit in small text, label under), then the seven sections of S5, each a plain heading in the flow: the title in bold and its question in secondary text on the same line, wrapping under it when narrow.
- Grid of boxes: two columns up to about 1,250 points of content width, three beyond, one under about 560. A lead box spans the row; the other boxes follow side by side and wrap. Boxes in a row share the row's height.
- A box is the system group box (`GroupBox`, the system's grouped background): no shadow, no custom border, radius or background.
- A box's content sits about 16 points inside the box on all four sides, as the touch group box and inset grouped lists place theirs (Antoine, 2026-11-23, on the Mac: "Not enough internal margin around this grey section"). The iPad's group box and N25.6's group already do; the Mac's group box keeps only about 4 points, so its content adds `Spacing.small`. Every part of N25.4 lines up on that inset, the header's switches and marker included; the strip and the section headings stay level with the box's outside.
- The iPad at regular width is this page in touch sizes, with the sidebar and two columns; in a narrow window or Slide Over it becomes N25.6's column.

### N25.4 The parts of a box

Top to bottom, at the box's inset:

1. The header: the title in the headline style, a plain heading with no chevron while the chart has no page of its own (Antoine, 2026-11-23: the carets served no purpose; decided by the coordinator under his delegation); once its page exists (N25.5, U302) a button with a trailing chevron (`chevron.right`) that opens it; trailing on the same line, the box's switches as small segmented controls (Reviews Over Time: By Type | By Answer and Count | Time; Retention: All | Young | Mature), or "Now" in secondary text for the charts of the present, or "Estimated" for Knowledge Over Time.
2. One large figure (the title-2 style, tabular digits) and its sentence in secondary text.
3. The plot (Swift Charts), at a fixed height per box size, y-axis labels on the trailing side as the system draws them, no second axis (S8.2). Bars have square corners and the parts of a stacked bar meet with no space between them, so they read as one bar; single bars match (Antoine, 2026-11-23, paraphrased: a zoomed stack did not read as one ordinary bar; decided by the coordinator). Bands of a stacked area meet too. With Differentiate Without Colour a line of about 1.5 points parts them (N25.12).
4. The legend, in stack order, each swatch with its name (the system chart legend).
5. Two to four small figures in a row (label in secondary text over its value), stacking at accessibility sizes.
6. A footnote in footnote secondary text when the box has one (left-out cards). Changed 2026-11-23 (Antoine: "There's a lot of white space here more than I expected"): a box keeps no line for a footnote it does not have, so its bottom inset equals its top one; the boxes of a row still share the row's height, and a box keeps its height across its phases. No box face mentions suspended or buried cards left out (Knowledge Over Time, Cards Over Time): that goes to a marker's help or the chart's page.

Every header is as tall as one with a small switch, so the boxes of a row line up. A month axis over more than one year writes each label with its whole year ("Oct 2028"), never a two-digit year (U297, 2026-11-23).

Colours: `stats/statistics-page` S8.1 (system colours only; the state family and the answer family, indigo for single measures, Forgotten orange). Selection, callout and Show in Browse: S8.3, S8.4; the callout is the system chart selection's annotation, a small material panel that never covers the rule. Empty and sparse: S8.5, the line centred in the plot area in secondary text.

Built 2026-11-23 (U299 to U301, PR #161; the implementer's readings, pending the coordinator):

- Card Counts (Antoine, 2026-11-23: a pie chart; then "Fill in the center of the donut", "Remove the xxx cards in xxx notes", "Remove the total and notes counters"): a full pie, 150 pt across (the regular plot's height), or 116 pt where that keeps it beside the table, else centred above the table; beside a taller box (Antoine, 2026-11-24: "there is just too much vertical height") the pie beside its table grows into the row's height, up to 320 pt (a chart page's plot height) and never into width the table needs, and the rows share the pie's height with a hairline between them; sectors flat and meeting (a 1° gap only with Differentiate Without Colour), nothing written on them. No figure line. The table's count and share columns are each as wide as their widest value, right-aligned in tabular digits, so a locale's "52,6 %" never runs past the box; a row's highlight reaches a little past it without widening it (a negative padding on the rows had pushed the box past its inset and clipped "Now"). Sectors keep their true angles. A highlighted state keeps its colour and the others fade as a chart's other bars do; the table's row gets a quiet rounded fill. The pointer highlights, a click or a tap pins and a second one clears, Left and Right move, Esc clears. Selecting stays the row's click: once U303 gives the rows Show in Browse (S7.7), it is a separate action (the double-click on the Mac, as a bar's, and a named accessibility action), never the click that selects. While loading, the pie is a grey circle with the progress indicator. A state with no card has a grey swatch and grey text.
- macOS overview exception (owner-approved, 2026-11-25; `stats/statistics-page` S7.7–S7.8): Card Counts and Added and Started stay side by side where the grid has room. This supersedes the preceding pie-growth rule on this overview: the pie takes about a third of the inner width, capped at 160 pt, independent of the neighboring box's height. Its seven table rows stay compact with aligned count/share and modest separators; stack the pie above them when needed, and let names wrap. Added and Started has four localized summaries above the trend (Added / Started / Added a Day / Started a Day), replacing the large figure and bottom figures. The wide plot is 120 pt, with the existing bars, line, legend, axes and interaction; narrow and accessibility text layouts keep the regular 150 pt height and adapt the summaries. Detail and mobile keep their earlier layout. Selection, hover, accessibility and export behavior stay intact.
- Answers (Antoine, 2026-11-23: percentages instead of counts): an ordinary chart box, three vertical bars stacked to 100% on a 0 to 100% axis with the system legend; no table under it, since the callout gives each answer's share and count and the kind's answers.
- Knowledge Over Time's likely forgotten band (Antoine's option 1, 2026-11-23): an area between Studied and Known, in Forgotten's system orange at the opacity a chart's faded bars take (30%, 50% with Increase Contrast), under the lines, broken where either line has no value, with straight joins. With Differentiate Without Colour it keeps its fill and adds no pattern: its edges are the two lines, which differ in more than colour (solid and dashed), and the legend and callout name it. The chart system's new band mark draws it.
- Time of Day's panels are 84 pt each, with y labels in one width so the two plots line up. The upper panel has no legend, x labels, callout, focus or hover of its own; the lower one holds the focus, the keys, the callout (which also names the hour's share remembered) and the per-hour elements. One legend row under both is drawn in the system legend's style. Its Audio Graph has the reviews series only, since an Audio Graph has one y axis and S7.11's panels never share one; each hour's element reads both values.
- Retrievability's desired retention is a dashed vertical rule, 1 pt in the label colour, at the value's place on the 0 to 100% bins (90% on the leading edge of the 90%–95% bar), named in the legend and in each bar's callout. Until its value arrives with the review log (S10.1) the legend keeps its entry and no rule is drawn.
- Reviews Due's axis: on an axis with Overdue or Later the labels are counted on from the current period, Overdue and Later take a label's place each, and a date within two thirds of the labels' spacing of either is not labelled. Today's label beside Overdue is therefore left out.
- By Deck is a plain table in the box, not a `Table`, which would scroll on its own inside the page and need a fixed height. Its column headers are buttons that sort (a chevron on the sorted column): the deck column starts in the deck tree's order, a number column starts largest first, and a row with no retention goes last either way. A row is a plain button with a quiet hover fill (no hover while the page scrolls); the deck's own-cards row is not a button. At compact width it shows the name, retention and time, and sorts by the deck order when its column is not shown. The sort is kept for the box's life, not stored.

### N25.5 A chart's own page

- In the same content column, replacing the page (a navigation push in the detail): the chart at full width and a taller plot, its switches, then its table as a native `Table` (Mac, iPad) or a list (iPhone) with one row per bar, then "About This Chart" (two or three sentences in body text) and, where there is one, a link button to the setting behind it. Retention's page adds By Period (`stats/graph-presentation` P12) under its table.
- Mac: the toolbar's back button and ⌘[ return; the deck and period controls stay in the toolbar and work. Touch: pushed, with the system back button.
- Context menu on a chart, on both pages: Copy Image, Copy Values.
- Screen contract `stats.chartPage`, added with its unit.

### N25.6 iPhone, the folded Duo, a narrow iPad (4A)

- The same sections in one column, on the grouped grey background (inset grouped style), each box a section-like group at the list's inset.
- The strip is four tiles in a row (Studied Today, Retention Today, Remaining Today, Streak); Mature moves to Card Counts' figures.
- Boxes keep their figures. Reviews Over Time keeps By Type | By Answer in its header; Count | Time moves into the box's menu (`ellipsis.circle` in the header).
- The Activity grid is N25.8's, at N24.6's touch squares: the column's width gives more, shorter lines; it never scrolls sideways (changed 2026-11-23: the swiping year strip is withdrawn).
- The Jump menu lists the seven sections; choosing one scrolls its heading to the top.
- The Duo held tall uses this column with the small boxes two abreast.
- Activity at this width shows its recent lines (decided 2026-11-23 by the coordinator, pending the owner's look: six years at phone width are about 17 lines): the lines that hold the last 53 weeks, rounded up to whole lines so they start at a line's first column and today stays at the bottom right; under the grid "Show All Years", then "Show Less", which grow and shrink the grid in place with today's line and the control held where they are on the screen; kept per device. A history whose recent lines hold every week has no control. The Mac and the iPad at regular width show every line.
- Built 2026-11-23 (U304, PR #162; the implementer's readings, pending the coordinator): the narrow page is the shell's tab layout (the iPhone at any width, the Duo folded and held tall, the iPad at compact width), not the size class, so the Duo held tall, whose size class can be regular, gets the grouped column with its small boxes two abreast (the grid's two columns from about 560 points). The title is inline, centred, with the subtitle under it; the deck pop-up is the bar's leading item ("beside it"); the sync button then the Jump menu (`list.bullet`, "Go to Section") are the trailing group, the sync button first as on Decks (N23.6). The period is the system segmented control across the column in a `safeAreaBar` under the navigation bar, so the bar's scroll edge effect runs behind it; it is shown, disabled, in the states that disable the controls. The strip's four tiles are section-like groups (`Radius.medium`): the value, its unit and detail in caption under it, the label under them; four in a row where four fit at about 72 points each (scaled with the text size), else two by two, else one per row. A box's small figures sit in one row where they fit at about 88 points each, else two to a row. A switch stays a small segmented control while the header fits it beside the title, else it is a menu showing its option (`ViewThatFits`); at the accessibility sizes it is always the menu. A callout is never wider than its chart: its lines wrap. With no collection on the device (first run) there is no Statistics tab, as Browse has no row (N18.3). Browse has no compact layout yet, so the bar reads Decks, Statistics, Settings.

### N25.7 The Duo opened (5A)

- The inner display (951 × 669 points, the fold down the middle, the side tab bar on the left) shows one page in two halves: each section is one screenful, its lead box on the left half and the other boxes on the right half; a section with one box (Coming Up, Decks) gives the plot the left half and its figures, footnote and table rows the right half.
- The deck pop-up is at the top of the left half, the period control at the top of the right. The two halves scroll together, section by section.
- Nothing interactive sits on the fold: no box, control or plot crosses it. The lead charts get the half's width (about 430 points): a year in weeks fits; All is drawn in months.

### N25.8 Activity with streaks (7A)

The Reviews section's second box: the grid of N24.6 (the Mac's 10 pt squares, touch 16 pt, the system green, AH7's levels) for all of history, then the figures under it. Hover (Mac) or touch shows a day's date and count. Nothing about streaks appears on Today.

Changed 2026-11-23 (the owner's Option B, `decisions.md` row `stats.overview` of that day, PR #162), on every device: Activity has a headline like the other boxes (N25.4 part 2), the current streak, "<n> days in a row" or "No streak today", with the sentence "Your longest streak · studied <d> of <D> days this year" or "Longest streak <m> days · studied <d> of <D> days this year" (S7.5); each year's total is on the grid after its year's label, "2026 · 87,634 reviews" with "· 87,634 reviews" in caption secondary text, shortened to "· 87,634", then left out, where it does not fit before the next label or the line's end; month names give way to it. Nothing is under the grid (the narrow page's Show All Years excepted): the four figures, All Reviews and the row of year totals of the bullets below are withdrawn. Built: the total sits 4 pt after the year; the years are placed first, then their totals, then the months; a year whose text had to end before its rule at the trailing edge gets none. VoiceOver: the grid's element reads the headline and sentence; a container per year, labelled "<year>: <n> reviews" (`stats.activity.yearTotal.<year>`), holds the month containers.

Changed 2026-11-23 (the owner's Option E, "Continuous, wrapped like text"; `decisions.md` row `stats.overview` of that day): the year blocks are withdrawn everywhere, the narrow layout's sideways scrolling and N25.6's swiping strip with them.

- One grid, a column per week with the seven weekdays down it, unbroken from the week of the first review to the current week; first weekday from the system, as on Today. The squares keep Today's size, corner radius, gap, colours and levels; today's square is outlined; days before the first review and after today are not drawn.
- No sideways scrolling, ever: the grid fills the box's width with as many week columns as fit and wraps onto a new line at the edge, like text. Oldest at the top left, today at the bottom right; the last line is usually partial and leading-aligned. The box is sized from its width in the same layout pass, so a window resize moves the line breaks live with no frame at a stale height.
- Each line has a label band above its squares (caption): at the column holding 1 January a thin rule (1 pt, the secondary label colour, from the band's top to the squares) and the year in caption semibold in the label colour, in place of January; other 1sts carry the month's short name in caption secondary text, as on Today. The grid's first column carries its year too, unless a 1 January's year is in its way; a year at 1 January is never left out (past the edge its text ends just before its rule). A later line that starts mid-month names that month when at least three of its columns before the next 1st are that month and the month step names it. Names that do not fit are thinned to every second, third, fourth or sixth month, or the years alone.
- Lines are 8 pt apart (from one line's squares to the next line's band), more than the 2 or 3 pt between rows, so lines read as lines; the band sits 4 pt above its squares.
- Under the grid, in rows that wrap: Current Streak, Longest Streak, Days Studied This Year and All Reviews (label in caption secondary text over the value), then each year's total in subheadline secondary text, oldest first; at the accessibility text sizes the four figures stack (S15). They never sit beside the grid, so the lines keep the box's whole width. Activity has no large figure (S7.5 gives none).
- A pinned day has a 2 pt outline in the label colour and the callout beside it (date, reviews), on the side with room, or under or over the day when neither side has room; never over the day, always inside the grid. Left and Right move a week, Up and Down a day (S14). Return or a double-click is the Show in Browse seam (U303). One accessibility element per day, inside a container per month; under right-to-left layout everything mirrors. The pinned day is kept as a calendar date.
- While its part has not returned, or after it failed, the grid area has its final size for the width, from the header's oldest review (S10.3), with the small progress indicator or the line that says why in its middle; each year's total reads "<year>: — reviews".
- Performance: the whole grid is one canvas, drawn again only when the counts, the width, the direction or the appearance change, never while the page scrolls or the pointer moves; hit-testing is by the grid's geometry, with no view per day. Today's own grid is unchanged (`ActivityGridUnchangedTests`).

### N25.9 Loading, kept results and status (decided 2026-11-22)

Antoine, 2026-11-22: the Statistics page "is a bit of an exception", "it's understood for users that this page can take a bit longer to generate than other pages"; it "needs to be performant", but reading hundreds of thousands of reviews cannot always be instant. Decided by Claude (coordinator) on his question whether to show a loading indicator, cache, and show that data is refreshing (`decisions.md`, 2026-11-22). Replaces round 11's "Loading: none visible" and "no spinner, nothing moves" for this page only.

- Tiers: the present state first, then the review-log charts, then Knowledge Over Time (`stats/statistics-page` S10.1).
- Kept results show at once and refresh in place, numbers changing without layout shift; while refreshing, the subtitle adds " · Updating…", then " · Updated just now" for a minute, or " · Updated 9:41" when the shown numbers are kept ones and the refresh could not finish. Never a blocking overlay.
- With nothing kept: every box at its final size with its title; a box whose data is not ready shows the system's small progress indicator (`ProgressView`, `.controlSize(.small)`) centred in its plot area, determinate where the core reports progress. No full-page spinner, no skeleton, no shimmer.
- No Refresh command and no pull to refresh: the page refreshes after changes by itself (S11).

### N25.10 States

`stats/statistics-page` S12: no collection (the shell's state, controls disabled; with no collection on the device the system's `ContentUnavailableView` "No Collection"), an empty collection ("No Cards", controls disabled), no reviews yet, sparse, a deck filter, a filtered deck (one line under the strip), a deleted deck ("This deck no longer exists." with Show All Decks), an unreadable record ("Statistics Unavailable"), a box that failed alone. Every screen unit checks first run and no collection.

### N25.11 Keyboard and menus (Mac, iPad with a keyboard)

- View › Statistics ⇧⌘S; View › Go to Section ▸ the seven sections (enabled while the page shows); Deck › Deck Statistics (a deck shown).
- Tab: the toolbar's deck pop-up and period control, then box by box (header, switches, plot). In a focused plot Left and Right move the selection one bar, Return runs Show in Browse, Esc clears. ⌘[ back from a chart's page. On the Mac a double-click on a bar runs Show in Browse.

### N25.12 Accessibility

`stats/statistics-page` S15. Each chart one element labelled by its sentence, with an Audio Graph and one element per bar in runs of ten; Increase Contrast and Differentiate Without Colour (separating lines in stacks); Dynamic Type to the largest size, legends and figures stacking, the plot keeping its height; the strip's figures one element each; identifiers `stats.*`. Activity is a grid, not a chart (changed 2026-11-23, as S15 says): no Audio Graph and no element per bar, but one VoiceOver element per drawn day (its date and count, "Today" first on today) inside a container per month, and the grid's own element reading the streaks, Days Studied This Year, All Reviews and the year totals. Changed 2026-11-23 (Option B, PR #162): the grid's element reads the headline and its sentence (the current and longest streaks, the days studied this year), and each year's total is its year's container label; All Reviews is not read, as it is not shown.

### N25.13 What this replaces

- N11.7: its second and third bullets (the scope picker with Deck, Collection and Search, the history picker and its labels, the compact subtitle "Japanese · Last 12 Months" and the toolbar menu): replaced by N25.2. Its first bullet (the header is the window's toolbar) and fourth (system group boxes, system animations) stay.
- `screen-inventory.yaml` `stats.overview`: its earlier text (the scope and history header, thirteen equal boxes in Anki's order, "the heatmap and streak stats are not here", the Stats sidebar section and tab names); and in `home.deckPage` "streak stats and the Stats action (U228) are not built" (the Activity header leads to Statistics; streaks stay off the deck page).
- N20.2's table gains Deck Statistics; N21.1's sidebar order gains Statistics second; N21.1's iPhone tabs become four.
- Unchanged: N11.7's toolbar header principle, Today's heatmap (N14.11, N24.6), round 10's "no streak on Today", Browse (Show in Browse uses its tokens; `search/filter-tokens` T3's Studied On token is also made by the page).

### N25.14 Scrolling (2026-11-23)

Antoine, 2026-11-23, paraphrased: after the first fill, scrolling should feel smoother; the first fill's time is fine. Checked by `StatsPerfTests` and `StatsTouchPerfTests` (R2078):

- Each box's view observes its own box: the model keeps one observable cell per box for its content and its selection, so a hover, a part arriving or a progress tick redraws only the box it changes. A plot's accessibility elements are made again only when its data changes, not on a hover.
- While the page scrolls (any scroll phase but idle), the pointer's hover selects nothing; a bar it was over when the page started to move is let go, a pinned bar stays.
- Nothing about the page's look changes.
- Measured (the hooks scrolling the platform scroll view in animated steps, the pointer over the page, the main thread's late time per second): the Mac on the 150k collection, settled 447 to 549 ms/s before, 0 to 37 after; while a refresh reads, 439 to 517 before, 43 to 288 after. The causes were the one dictionary of box contents and the one of selections every box read: a hover change, a part or a progress tick redrew all fifteen charts. Marks were not a cause: at All on three years no chart draws more than 185. XCTest's own hitch metric reports nothing in the test VM, so the tests assert R2078 on this measurement.
- Changed 2026-11-24 (decided by Claude (coordinator) under Antoine's delegation; the Statistics follow-up PR #192): the page model writes the boxes' values off the main actor when the core's parts and the request's end arrive, and publishes the boxes of one write together once all are written; a box shows its earlier values until then, and the status stays Updating while they are written, and a newer write of a box supersedes one still being written. The user's own changes (a switch, the period, the deck) still write in the frame they cause (S19).
  - Measured with that change on the 150k collection (`StatsPerfTests/testScrollDuringRead`, five scrolls a run, the same test VM): while a refresh reads, 259 to 287 ms/s, and 260 to 410 once a refresh's start also wrote off the main actor; settled unchanged. The page model's writes were not what kept the ratio over R2078's 5 ms/s; the cause is still open (`docs/process/verification-status.md`).

## N26. The home screen on the Duo, opened and folded (2026-11-22)

From Antoine's annotations on a capture of his opened iPhone Duo (`decisions.md`, row `home.deckList` of 2026-11-22): "Why squared not round?", "Too much white space" and "Should we have a delimiter between the top links and the deck?". Status: built in PR #122. N26.3's variant A was chosen by the coordinator under Antoine's delegation (2026-11-22); variant B stays the alternative he may still pick. Where earlier text differs, this section wins.

### N26.1 The selection in the list beside Today

- The deck list beside Today (the Duo opened and held wide, N14.10) is a List with a selection: the Today row and every deck row are tagged, and the List draws the selected row with the system's own highlight, rounded and inset, as the iPad sidebar does. The rows' buttons still navigate; the selection only shows where the detail is (Today, the deck whose page shows, or the collapsed deck that hides it).
- Cause of the square highlight: the rows were buttons in a list with no selection, so the selected one was painted with `listRowBackground` and the system's grey cell fill, which fills the whole cell. `lint:painted-selection` now refuses a selected row painted that way.
- Replaces N14.10's "has the system's grey selected-cell fill". Its number still takes the primary label colour while the row is selected.

### N26.2 Today's totals are the title's subtitle

- On touch (the iPad's Today and Today beside the deck list on the Duo), the totals ("24 new · 3 learning · 75 to review") are the navigation title's subtitle (`navigationSubtitle`): directly under the large title, on its leading edge, in the system's subtitle style. The page's first group follows at the system's distance, and the title, the subtitle and the groups share one leading edge. Nothing shows while nothing waits, as before. The Mac keeps N14.4's toolbar subtitle, unchanged.
- Cause of the space: the totals were a section of their own in the inset grouped list, with a clear background: a row inset from the group's edge, with a section gap above and below.
- Replaces, on touch, N14.1's and N24.2's "the title and the totals line" as a list row; `home.today.totals` is gone (the subtitle is the navigation bar's text).

### N26.3 Top destinations and the decks

- Built (variant A, chosen): Today is a section of its own, then the decks in a section with the system header "Decks", as the iPad sidebar has (N14.6). The column draws no title (N26.6). The tab is still called Decks.
- Variant B: the same two sections with the system's section spacing only, no header, and the title "Decks".
- Recommendation and reason: Apple's sidebars with fixed destinations above the person's own list put a header over that list: Mail (Favorites, then each account), Notes (the account's folders under its name), Reminders ("My Lists" under the smart lists), Files (Locations, Favorites, Tags). A header also matches the iPad, so the Duo opened and the iPad read the same.

### N26.4 Search beside Today

- Beside Today, the column always shows the system search field under its title (the navigation bar drawer's `.always` display mode), so the toolbar's Search button (N14.2), which only presented that field, is not drawn there. The compact deck list (an iPhone, the Duo folded or held tall) keeps the button, since its field stays out of the way until asked for.

### N26.5 Deck rows are one height (Antoine, 2026-11-22)

- Every touch deck row is as tall as a row without subdecks (52 points at the default text size), with or without the leading disclosure (N14.7), in the tab layout's list, beside Today and in a deck page's Subdecks; the Today row beside the list too. The row's button keeps its 44-point minimum as the row's content, and the list's vertical padding around it is `Spacing.xxSmall`, scaled with the text (`listRowInsets(.vertical, …)`); the horizontal insets stay the system's. The disclosure's 44 × 44 target is unchanged and still reserves no layout.
- Cause of the mismatch: the 44-point minimum sat on rows with a disclosure only, and the list added its own vertical padding around it, so those rows were about 73 points against 52.
- iPad and Mac sidebars draw the system's own rows, which already share one height; unchanged.
- Checked by UI tests in the test VM.

### N26.6 No title over the list beside Today (2026-11-22)

- Antoine, on the opened Duo: "Should we remove the big Meguri text on the top left?"; the coordinator decided yes. The column beside Today draws no title: its navigation bar holds only the trailing buttons (sync, Add) and the always-visible search field follows directly, with the inline title mode so no large-title band is left (`toolbar(removing: .title)`, as the iPad sidebar, N14.9). "Decks" stays the column's navigation title for VoiceOver and the scene; "Decks" is drawn once, as the section header (N26.3).
- Single-column layouts (an iPhone, the Duo folded or held tall) keep the large title "Decks" (N14.8) and have no "Decks" header, so the word shows once there too. The iPad sidebar draws no title (N14.9); the Mac sidebar never drew one. Both unchanged.
- Replaces N26.3's column title "Meguri".

## N27. Home at launch: no flash of a false state (2026-11-23)

Antoine's report from his iPhone Duo (2026-11-23), paraphrased: started cold, the app shows the screen that says there are no decks for a split second, then the decks; should it show a blank screen until everything is loaded, or a loader, and what is best. His standing rule: every interaction instant, no loading screen that flashes, old content stays until new content is ready, preload with pure reads. Decided by Claude (coordinator) under his delegation, pending his look (`decisions.md`, 2026-11-23). Where earlier text differs (N2's distinct collection-loading state, N14.1's "Loading … keep their content"), this section wins.

Cause: the open set the collection ready before Home's first deck tree was read, and Home took "ready with an empty tree" for a collection with no decks. Between the two, the host completion awaited the sync attachment, the previous-session record, the day check, the media index and study preparation, in that order, before the tree read; the page drew "No Decks" (the sidebar "No decks", the opened Duo one column) for that time. Before the open, Home drew "Opening collection…" with a spinner.

### N27.1 What each state shows

- The welcome shows only when there is no collection to open (`state` no file). "No Decks" (and the sidebar's "No decks") shows only after a finished read of the open collection found no deck.
- Not known yet (unopened, opening, open with its tree not read) is its own state (`HomePhase.pending` or `.kept`): it never draws the welcome, "No Decks" or a loading screen with text. An exclusive phase before any read shows the kept read when there is one.
- A first read that failed is known: the error with Try Again, never a wait that does not end. Failed, busy (an exclusive phase, which never brings the kept read back after a read) and Start Empty's recovery keep their own views.

### N27.2 The kept read

- At the end of each successful read Home keeps what it showed: deck names, the tree's shape and its saved disclosures, the counts, Today's figures (Studied Today, the next learning card, the previous session) and the tab layout's previous-session count, with the study day's end (`next_day_at`). One small file in the app's caches (`<Caches>/Meguri/home-snapshot.json`), excluded from backups, never synced, written off the main thread when it changed.
- It names its collection file (file number and creation time; not the path, which moves with the app's container on an update), and its counts' day end is the one known when they were read. A launch reads it before the first frame and shows it only for that same file: a replaced collection (an import, a download, a restored backup, a fixture) is another file, so its snapshot is deleted unread. It is also deleted when a replacement starts or happens in the app (an import, a Meguri Sync or iCloud download, a manual transfer), and when no collection file exists. The app has no sign-out or reset that keeps the file; those that replace or remove it are covered by the rules above.
- Until this launch's first read the deck list, the sidebar, Today and a restored deck page show the kept read; the read replaces it in place: same rows, same identities, same order, no layout jump and no scroll reset (the list keeps one view and its saved position). The heatmap and other reads wait for the first tree (N27.5).

### N27.3 Held counts and nothing kept

- Kept counts from an earlier study day (the day they were read in has ended, or its end is unknown) are held back: rows show names and the tree with no number (blank, never zero), nothing speaks a count, the Mac shows no subtitle, and Today and a deck page wait for the read in their own background. Learning cards that came due during the same day are not a new day: those counts show as kept and update with the read.
- With nothing kept (the first run after install, after a replacement) and a collection still opening, the shell shows its chrome and an empty content area in the page's own background, with no spinner and no text. Only when the read takes longer than half a second does the system's small progress indicator (`ProgressView`, small) show, centred; it goes the moment content arrives. Never a full-screen loading screen.
- On the first launch after install there is no collection file: the welcome shows as soon as the open finds none, in that background before it.

### N27.4 A press on kept content

- Kept controls look as they will (Study is not dimmed) and never act on kept ids. A deck row, a sidebar row or Study pressed before the collection is ready waits for the first read and the open's end (a few hundred milliseconds, at most three seconds), then acts on the read tree: a deck opens only if the read tree has its id, Today's Study walks the read decks. A later press or any navigation meanwhile supersedes the waiting one, and a deck the read tree lacks does nothing. Chosen over inert controls: the press is never dropped while the open is that short, and the list beside Today shows its selection at once.

### N27.5 Nothing else before the first tree

- The first deck tree is read as soon as the open's day check has run, before the sync attachment, the media index and study preparation; if changes were committed meanwhile, the open reads the tree once more at its end. Recovery drops that arrive after the session record was read apply as they come. The snapshot is not written during a review (its end writes it). The heatmap's reads wait for it (the core's query lane is serial; a history scan queued first delayed the tree). R1818's interval `launch.toDeckListInteractive` runs from the collection owner's creation to the first tree.
- Measured with `launchTimingFirstDeckTreeIsReadBeforeTheRestOfTheHostCompletes` (AppModelTests) on the generated 10k collection (this Mac, Debug build, other jobs running, median of five opens): the core open takes about 200 ms. From the ready state to the first tree took 4.7 ms before and 3.7 ms after. The moved steps are short in that test, which has no sync service; on a device the sync attachment runs in that window. With a kept read the decks are in the first frame and nothing waits on screen.

### N27.6 The opened Duo's layout

- A launch that found the collection's file (or a kept read) lays the opened Duo out as the list beside Today from its first frame, instead of one column that switches to two. With no file it keeps N18.3's single column for the welcome.

### N27.7 Launch screen

- The touch launch screen is the system grouped background, light and dark (`UILaunchScreen` with the `LaunchBackground` colour, #F2F2F7 and black), the first frame's: the deck list, Today (inset grouped, N24.5) and the welcome are grouped lists. It was the generated empty launch screen, white in light mode, which showed as a white frame before the grey list. No bars are drawn: the system's launch-screen bars do not match iOS 27's glass bars, which appear with the first frame.

### N27.8 What this replaces

- N2's "distinct collection-loading state" and the "Opening collection…" loading view on Home; N14.1's loading state for Today. The failed, unavailable, no-file and empty-deck states stay distinct.
