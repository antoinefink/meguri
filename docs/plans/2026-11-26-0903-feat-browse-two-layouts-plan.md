---
title: Browse in two layouts - Plan
date: 2026-11-26
type: feat
execution: code
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-plan-bootstrap
---

# Browse in two layouts

## Goal Capsule

**Objective.** A Meguri Sync member can scan many cards at full width in the web account's Browse page, then open one card with enough room to read and edit it while keeping their place in the results.

**Means.** Restyle the existing Browse page into a full-width result list that becomes a narrow rail when a card or new note is open, using the current server operations unchanged (KTD1, KTD2).

**Authority.** This plan's Key Decisions and Requirements, then [W12 in the web specification](../../specs/servers/meguri-sync-web.md#w12-browse-the-signed-in-collection), then the [online Browse plan](2026-11-25-1512-feat-online-browse-plan.md) for behavior this plan does not change. The approved mockups live on a private design canvas (https://claude.ai/artifact/KPgZ9bEqs9FoYZ7PpsfD8Y, boards 1–9); R1–R9 are their authoritative summary.

**Stop conditions.** Stop if the layout cannot keep an existing safety behavior (dirty-draft guard, stale-response discard, revision-checked save, exact sibling-set delete confirmation, account isolation). Stop if a requirement needs a server API change; those items are deferred.

**Execution profile.** One pull request against `master`, web client and spec text only. The agent implements, verifies through `tools/mg`, and opens the PR; Antoine merges.

## Product Contract

### Summary

Browse shows results as a full-width list with column-aligned rows until a card opens. Opening a card or adding a note turns the list into a narrow rail beside a wide card area with previous/next stepping. Search matches get their own column, deletion confirms in a modal dialog, and a concurrent change shows a banner in the card area. Phones show the list and the card as separate full screens.

### Problem Frame

The current page keeps a fixed 2:3 split. The search form fills half of the list column, each result spends three lines, and the editor squeezes raw field text into narrow boxes. Browsing wants width for scanning; editing wants width for reading. A permanent split gives neither.

### Key Decisions

- Two layouts on wide screens: a full-width list with no card open, a rail plus card area with a card open. (session-settled: user-approved — chosen over the current fixed 2:3 split and the compact-split, table-with-inspector and deck-column mockups: browsing and reading or editing a card each need the room.) Governs R1, R2, R3, R4, R5.
  - Conflict call-out: the approved card-open mockup showed no search toolbar above the rail. KTD9 keeps the toolbar visible there on wide screens so search and filters stay usable while a card is open, which the existing stale-read and account-isolation tests exercise.
- The remaining states follow the approved mockup set: a Match column while searching, Add note in the card area, a modal delete confirmation, a conflict banner in the card area, and full-screen list and card on phones. (session-settled: user-approved — chosen over today's stacked search form, inline delete confirmation region and stacked mobile panes: they follow from the two-layout model.) Governs R2, R6, R7, R8, R9.

### Requirements

**Browsing**

- R1. With no card open, the result list uses the full content width under a toolbar: search field with a submit and a clear control, Field and Deck selects, and Add note, with the result status on its own line beneath.
- R2. Each result row aligns Note title, Card template and Deck in columns; while a query is active a Match column shows the matched field name and the excerpt with the match marked.

**Card open**

- R3. Opening a card or Add note shows the results as a rail of titles and templates beside a wide card area, with the open card marked; an All cards button, and Escape pressed anywhere in the card area except the search box, selects and dialogs, return to the full list with query, filters, scroll and row focus restored.
- R4. The card header holds previous and next card controls, the title, a "template card · deck" line with the sibling count, a delete control, Load server copy and Save; ↑/↓ outside text entry step to the adjacent loaded card through the same unsaved-changes guard as clicking a row.
- R5. The card area shows the note fields and tags at a readable width, with Card info (card ID, note ID, template, deck) always visible beside or below them; fields whose stored text holds markup or media references use a monospace face.

**Other states**

- R6. Add note shows Deck and Note type side by side, the type's fields and tags, and Cancel and Create note in the card header, with the rail kept.
- R7. Delete note opens a modal confirmation naming the note and its exact card count, with Cancel focused; Escape cancels. A failed delete keeps the dialog open and shows the error inside it; a conflict closes it and shows the R8 banner.
- R8. A concurrent edit shows an error banner in the card area that keeps the draft, offers Load server copy, and leaves Save and Delete disabled.
- R9. Below the rail breakpoint the list and the card each take the full content area. Phone result rows keep the match field and marked excerpt while searching. The card keeps All cards, previous/next and Delete at the top, and Save with Load server copy in a bar that stays at the bottom of the card column while the form scrolls; Add note keeps Cancel at the top and Create note in that bar.

### Scope Boundaries

- Behavior of paging, search matching, writes, conflicts, account isolation and focus restoration stays as W12 and the online Browse plan define it.
- No server, API or MCP change.

#### Deferred to Follow-Up Work

- Card Preview: rendering the note type's templates and styling beside Edit.
- A second-field column in the list and a second line on phone rows when no search is active (needs a preview field per row from the page operation).
- Named sibling cards ("Cards from this note") and the cards a note type will create in Add note (needs template names from detail and catalog).
- The server's newer field values inside the conflict banner (needs the conflict response to carry them).

## Planning Contract

### Key Technical Decisions

- KTD1. One result list DOM serves both layouts. The list stays a `ul` of row `button`s carrying `data-card`; CSS grid lays out columns at full width and hides the Deck cells in the rail, which keeps the match line while searching. The full-list scroll offset is saved only while no card is open, so rail scrolling cannot overwrite it; when the rail appears and after each step the open row scrolls into view (nearest), and All cards or Escape restores the saved offset and then keeps the focused row visible. (Implements R1–R3.)
- KTD2. Layout is chosen by CSS from a page state class plus viewport width, not by separate React trees. `browse-show-detail` already marks card or Add note state; a `min-width: 75rem` media query picks the rail (17rem wide) versus the full-content card. The Browse page lifts the account inset's 72rem cap and uses 2rem inline padding so the list and card get the window's width (about 1000px of content and a 720px card area at a 1280px window, about 920px and 640px at the breakpoint). Narrow behavior reuses the current compact path (`back()`, scroll and focus restore).
- KTD3. Column headers are a visual row hidden from assistive technology. In the full-width list each row button's accessible name reads title, template, deck and match in order; in the rail it reads title and template, plus the match while searching. A full ARIA grid would need roving focus that the current keyboard model does not have.
- KTD4. The delete confirmation uses a native `dialog` opened with `showModal()`, so focus containment, Escape and the backdrop come from the platform. The existing `remove()` guard conditions stay unchanged; a non-conflict failure renders a `role="alert"` message inside the dialog, and a conflict or identity change closes it.
- KTD5. Stepping uses the nearest adjacent loaded card without `unavailable`; previous or next is disabled when none exists in that direction (stepping does not load more pages). A card opened by previous/next keeps focus on the pressed control, and one opened by ↑/↓ focuses the card heading; only a row click keeps today's focus on the matched field. Key handling sits on the Browse page element; it ignores ↑/↓ from inputs, textareas and selects, and Escape only from the search box, selects and an open dialog.
- KTD6. Field editors stay textareas sized to their content with `field-sizing: content` and min and max heights; the monospace style is chosen from the server copy of the field (`detail.note.fields`), not the draft, so it does not flip while typing.
- KTD7. The conflict message moves from the page-level outcome into a card-area banner with its own Load server copy button; while the banner shows, the header's Load server copy is hidden so only one exists. Page-level outcomes keep saves, deletions, refresh failures and account changes.
- KTD8. Search submit stays a real form submit: the magnifier becomes the icon-only submit button named Search, and a Clear search button appears while the field has text and clears the query and scope together.
- KTD9. The toolbar stays visible above the rail and card area in the wide card-open and Add note layouts, and is hidden only with the full-content card below the breakpoint. The search field flexes with a minimum of about 12rem, each select is capped at about 12rem with ellipsis, Add note stays at the row's end, and the status line sits on its own line under the toolbar so its changing length never shifts the controls. Below the breakpoint the search field takes the full width and the selects share the next row.

### Assumptions

- The 75rem breakpoint and 17rem rail leave the card area at least about 640px wide; narrower windows use the full-content card.
- "All cards" replaces the "Back to results" label everywhere; tests and spec text follow the new label.
- The sibling count line ("2 cards share this note") stays as text in the card meta line, since sibling template names are deferred.

### High-Level Technical Design

Directional sketch of the page states (CSS chooses the arrangement; React state is unchanged except where noted):

```text
                 wide (≥ 75rem)                                narrow (< 75rem)
no card open     toolbar + full-width list                     toolbar + full list
                 (Note | Card | Deck [| Match])                (title + template, deck; match line while searching)
card open        toolbar; rail (titles + templates) | card     full-content card, sticky bottom bar
add note         toolbar; rail (titles + templates) | form     full-content form, sticky bottom bar
delete           <dialog> modal over current layout            same
conflict         banner at top of card area                    same
```

Transitions: row click, ↑/↓ or previous/next → `open(card)` (guarded by `mayLeave`); All cards or Escape → `back()`; Add note → `add()`; Cancel → `back()`.

### Sources

- `server/web/src/browse.tsx`, `server/web/src/style.css`, `server/web/src/ui.tsx` — current Browse implementation and primitives.
- `server/web/tests/browser/browse.spec.ts`, `server/web/tests/browser/browse-real.spec.ts` — fixture and real-server browser tests that pin current labels.
- `specs/servers/meguri-sync-web.md` W12 — product rules for Browse.

## Implementation Units

- U1. Two-layout list and toolbar
  - Goal: Full-width column list and toolbar; rail styling when a card or Add note is open.
  - Requirements: R1, R2, R3, R9 (KTD1, KTD2, KTD3, KTD8, KTD9).
  - Files: `server/web/src/browse.tsx`, `server/web/src/style.css`, `server/web/tests/browser/browse.spec.ts`.
  - Approach: Replace the stacked search form and Card wrapper with a toolbar row, a status line and a list header row; render row cells as spans placed by a shared grid template; add the All cards button (the existing `back()` Button, relabelled) to the rail head; move the page heading beside the toolbar. Keep `.browse-results` as the scrolling element in every layout and handle scroll per KTD1. An unavailable row's explanation fills the Match cell at full width and becomes a further line under the title in the rail and on phones.
  - Test scenarios: at 1280×900 with no card open the list is wider than 900px and shows Note, Card and Deck headers; searching "aprovechar" shows a Match header and a marked excerpt in the row; opening a card shows the rail with the open row marked `aria-current`, hides Deck cells and keeps the search field visible; with 50 rows, opening a lower row keeps it visible in the rail, and All cards restores the earlier scroll offset with that row focused and the query kept; Clear search empties the field and reloads all cards; Enter in the search field still submits; at 390×844 a searched row shows its marked excerpt.
  - Verification: `tools/mg sync-web-check` and the Browse fixture tests through `tools/mg sync-web-verify`.

- U2. Card area, stepping and keyboard
  - Goal: Card header with previous/next, meta line, delete, Load server copy and Save; visible Card info; content-sized fields; narrow bottom bar.
  - Requirements: R3, R4, R5, R9 (KTD5, KTD6).
  - Files: `server/web/src/browse.tsx`, `server/web/src/style.css`, `server/web/tests/browser/browse.spec.ts`.
  - Approach: Build the header from existing handlers (`open`, `back`, `save`, delete trigger); Save submits the note form through the `form` attribute; add the page-element key handler and stepping focus per KTD5; replace the Card info disclosure with a definition list; mark code-like fields per KTD6. Below the breakpoint, the bottom bar uses `position: sticky; bottom: 0` inside the card column, so it never covers the account sidebar between 48rem and 75rem.
  - Test scenarios: Next opens the following loaded card, keeps focus on Next, and Previous is disabled on the first; with "aprovechar" searched, pressing ArrowDown twice from the heading reaches the third card, while ArrowDown inside a textarea does not step; Next skips an unavailable row; with a dirty draft, Next asks to discard and dismissing keeps the draft; Escape outside a field returns to the full list; Card info shows card and note IDs without a click; a field holding `<b>` markup renders with the code class; at 390×844 and 900×800 the bottom bar stays visible after scrolling the form and does not overlap the sidebar.
  - Verification: Browse fixture tests through `tools/mg sync-web-verify` at 1280×900, 900×800 and 390×844.

- U3. Add note, delete dialog and conflict banner
  - Goal: New-note form in the card area, modal delete confirmation, card-area conflict banner.
  - Requirements: R6, R7, R8, R9 (KTD4, KTD7).
  - Files: `server/web/src/browse.tsx`, `server/web/src/style.css`, `server/web/tests/browser/browse.spec.ts`, `server/web/tests/browser/browse-real.spec.ts`.
  - Approach: Add note keeps the rail and uses a header with Cancel and Create note (Create note moves to the bottom bar below the breakpoint); Deck and Note type share a row. The delete trigger opens a `dialog` holding the existing title, card-count text and Cancel and Delete note buttons; closing it returns focus to the trigger. The conflict state renders a `role="alert"` banner above the form with Load server copy; Add note's conflict keeps its existing text and Reload Browse action inside the banner.
  - Test scenarios: Add note shows the rail and the new-note header, and Cancel returns to the full list; Create note stays disabled until deck and type are chosen; Delete note opens a modal dialog naming the note and "2 cards", Cancel is focused, Escape closes it with no write, and Delete note sends exactly one delete with the shown confirmation; a failed delete request shows its error inside the open dialog; a stale save shows the banner, keeps the draft, disables Save and Delete, shows exactly one Load server copy, and that button replaces the draft after the discard prompt; the real-server compact flow still deletes both siblings and returns to two fewer rows.
  - Verification: fixture and real-server Browse tests pass through `tools/mg sync-web-verify`.

- U4. Spec wording (deferred to a spec-writer session)
  - Goal: W12 describes the two layouts. Specs are written only in the spec-writer role, so this implementer change leaves W12 as it is; the two layouts already keep the result list beside the editor as W12 requires.
  - Requirements: R1, R3, R9.
  - Files: `specs/servers/meguri-sync-web.md`.
  - Approach: Replace "The desktop page keeps the result list beside the note editor" with the full-width list that becomes a rail beside the open card, and name All cards as the compact return.
  - Test expectation: none -- documentation only.
  - Verification: the sentence matches the implemented behavior.

## Verification Contract

- `tools/mg sync-web-check` (typecheck, unit tests, production build) after each unit.
- `tools/mg sync-web-verify` for the fixture and real-server browser and axe suites after U1–U3 and at the end; a failure on a loaded machine is re-run alone before it counts.
- `tools/mg static-checks` for trailers and session records before pushing.
- Render the built page at 1280×900, 900×800 and 390×844 in the browser fixture and check each state against R1–R9; at most three render-and-fix rounds.

## Definition of Done

- R1–R9 hold in the built web app at desktop, mid and phone widths.
- Every existing Browse browser test passes, with locators updated only where this plan changes a label, role or element structure, and with no scenario or assertion removed or weakened.
- No server, API or dependency change; no new retries.
- W12 wording matches the behavior.
- Commits carry `Role:`, `Spec: servers/meguri-sync-web#W12` and a recorded `Session:` trailer.
- No abandoned experimental code remains in the diff.
