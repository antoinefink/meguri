# Native interface contract

This contract defines Antoine's agreed macOS direction for the current native preview.
Source: Antoine's approved HTML preview and feedback on 2026-10-24.
The HTML specifies hierarchy and behavior. It does not specify native control measurements.

## N1. Scope and authority

- Use native SwiftUI controls, semantic styles, SF Symbols and macOS menus.
- Keep the existing collection, review, import, export and server-transfer behavior functional.
- Keep iOS and iPadOS source paths compatible. This preview targets macOS.
- Antoine's newer direction overrides R6, R109 and R110 only for the separate native macOS Settings scene.
- Quiet corners overrides R21's option to reveal the sidebar during review. Restore the deck workspace after review.
- Preserve the underlying navigation and saved-answer semantics of R5, R12, R13 and R22.
- This bounded UI task does not resume the full application goal.
- Build, open and smoke-check this preview now. Broad gates follow visual agreement; do not claim unit acceptance.

## N2. Deck workspace

- Deck selection is the main workspace. Show the real collection's deck tree and available counts.
- Make Study the selected deck's primary action. Use `ReviewCollection.startStudy(deckID:)` and its existing availability guard.
- Center the selected deck and empty state in the available detail pane. Overflow content remains scrollable; fixed top padding is not the centering mechanism.
- On Mac, use a content-sized Study button and simple deck-name/due-count sidebar rows. The selected deck shows the three labeled counts without filling the sidebar with that full sentence.
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
- Preserve the separate media decision and its bidirectional effects, even when collection direction is one-way.
- Keep partial, cancelled, interrupted, excluded-media and unknown-remote outcomes distinct from success.
- Opening Settings or loading offline history must not start or resume a server request.
- Keep collection file export independent of account login. Preserve its destination lease and stale-generation safeguards.
- Do not rename collection replacement as safe incremental sync or imply both sides are up to date.

Supporting requirements: R1496–R1504, R2046, R2048–R2050, R2054–R2063, limited to the behavior currently implemented.

## N9. Pending backend work

The agreed destination is collection file export with media. Its prepared backend patch is paused and unverified.
Use plain Export Collection labels. Explain before export that the active file export excludes media.
The UI must describe the implemented export. Backend media work and its checks remain separate.
This limitation does not remove the existing explicit server media-transfer workflow.

## N10. Native preview evidence

- Use `tools/mg` for the focused native build and smoke checks. Use the macOS VM for UI automation.
- Inspect the exact build with a real collection path or explicitly identified local fixture; never present fixtures as live results.
- Inspect empty, normal, busy, error and long-content states at normal and smaller window sizes.
- Check light/dark appearance, larger supported text, focus, scroll boundaries and plain labels.
- Preserve named accessible actions, contrast and platform hit-target limits (R1977, R1979–R1982).
- Apply R1987's maximum three self-directed render/fix rounds before critic and gates.
- Native preview evidence does not replace the later full suites, independent review, provenance checks or human acceptance.
