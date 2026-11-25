---
title: Browse the synced collection online
date: 2026-11-25
type: feat
execution: code
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-brainstorm
---

# Browse the synced collection online

## Goal Capsule

**Objective.** A signed-in Meguri Sync member can find a card by a word inside its note, correct it in the browser, and see the correction on their device after sync.

**Product authority.** Antoine approved layout A and the field-search sketch on 2026-11-25. This contract expands the original browser scope in [Meguri Sync web accounts, W1](../../specs/servers/meguri-sync-web.md#w1-independent-policy-and-interface-scope) to include Browse. There are no open product blockers.

## Product Contract

### Summary

Add Browse to the signed-in account sidebar. It shows the cards in the member’s server collection and supports field search, quick edits, note creation and deletion. Card info helps members identify the exact card the server holds.

### Key decisions

- **Split view on desktop.** Governs R2. (session-settled: user-directed — chosen over a separate desktop detail page: the results stay visible when moving between cards.)
- **Search note fields first.** Governs R3–R5. (session-settled: user-approved — chosen over all-fields search alone: narrowing to Spanish removes matches found only in example sentences.)

### Requirements

**Find a card**

- R1. With empty search and default filters, show every card in the signed-in member’s server collection, including separate rows for cards generated from the same note.
- R2. On a wide screen, keep the result list beside the selected card’s editor. On a narrow screen, Back from detail restores the query, filters and scroll position.
- R3. Search for literal text within any note field by default, regardless of letter case or which fields appear in the list.
- R4. Let the member narrow the same query to a named field or a deck, or combine both filters. A note type without the selected field contributes no matches.
- R5. Each matching card appears once and shows the matching field name and a short excerpt with the matching text highlighted. Selecting the row brings a matching field into view.

**Inspect and change the collection**

- R6. Show the selected card’s ID, note ID, template and deck in Card info.
- R7. Let the member edit note fields and tags, and show when sibling cards share the note. Saving preserves the search, filters and position in the results that still match.
- R8. If a newer change conflicts with the open edit, keep the member’s draft available and explain that the server copy changed.
- R9. Add note lets the member choose a deck and note type, fill in that type’s fields, and create the cards it generates.
- R10. Delete note asks for confirmation and names the note and number of cards it will remove.
- R11. The member’s devices receive saved additions, edits and deletions through normal Meguri Sync.

### Acceptance examples

- AE1. **Find a Spanish word.** The sample has three cards headed *el viaje*, *aprovechar* and *la oportunidad*. Each contains “aprovechar” in its Example field. Searching all fields returns three cards. Choosing the Spanish field returns one. The *aprovechar* card appears once even though two fields match. Covers R1, R3–R5.
- AE2. **Improve a sentence.** Select *el viaje* from those three results. The editor shows “Quiero aprovechar el viaje para visitar a mi hermana.” Correct the sentence and save. The query stays in place, the excerpt reflects the edit, and the device receives it on the next sync. Covers R2, R5, R7, R11.
- AE3. **Compare siblings.** Create a note whose template generates Forward and Reverse cards. Both appear in Browse with distinct card IDs and a shared note ID. Delete note names both cards in its count. Cancelling keeps both. Confirming removes both and the deletion reaches the device on sync. Covers R1, R6, R9–R11.
- AE4. **Handle a concurrent edit.** While the web editor is open, sync a correction from the device to the same note. Saving the older web draft reports the conflict, preserves the draft and leaves the device’s correction intact. Covers R8.

### Scope and reuse

This release covers individual cards and their notes. Bulk editing, the full native query language and study sessions are outside its scope.

The Rust server already supports filtered note search, deck and note-type lookup, note creation, and revision-checked edits. Its existing search requires a filter and returns notes. Planning should assess reuse for the card list, field-specific matches, deletion and browser-session access. The server currently depends on the shared model crate. [Collection operations](../../specs/servers/meguri-collection-mcp.md) · [Server dependencies](../../server/Cargo.toml) · [Native Browse deletion behavior](../../specs/browser/workspace.md)

## Planning Contract

The web app uses its existing signed-in account session. Resolve the collection from that session on every request and require the displayed account ID as a precondition. Discard responses from a previous identity. Use the same origin, CSRF, body limits, worker admission and session-revocation checks as other collection actions. Recheck the account, active collection publication and expected revision at the point of use. Keep the MCP tool catalog and native sync protocol stable.

A bounded card-ID-ordered page reads from the published collection. Resume after the last examined card, including pages with no matches yet. Bind continuation to the account, publication, data epoch and filters. Bind open create forms and write revisions to the same collection generation. Search visible field text as data and highlight literal matches without executing card markup. Keep string IDs exact. Resolve display names from stored deck, template and note-type records. Populate field and deck choices using the collection catalog.

Reuse the existing note write transactions and revision checks through a browser authority adapter. Guard deletion with the same collection lock and an exact check of the sibling set shown in the confirmation. Write note and card deletion records with server sync stamps in the same transaction, updating an existing deletion record to the current USN if an ID was reused. Keep unrelated cards, media, scheduling and field markup intact.

## Implementation Units

- U1. Extend the account web specification and deliver browser Browse operations in the Rust server. Add account-scoped catalog, card page and detail reads, plus create, edit and delete. Exercise them through the real HTTP/account boundary with generated collections. Covers R1, R3–R11.
- U2. Add Browse to the React account shell and protected routes. Provide query and filters, bounded result navigation, highlighted excerpts, responsive list/detail, Card info and note forms. Ignore out-of-order reads. Preserve dirty fields across a conflict and protect them when changing selection. Covers R1–R10.
- U3. Verify the web and native-sync paths together. Drive the built website against the isolated server with synthetic fixtures, including a compact width. Confirm normal sync carries note and card changes and deletion records. Covers AE1–AE4.

## Verification Contract

Run focused server cases through `tools/mg sync-server-test` and run `tools/mg sync-web-verify` for the compiled web app. Check keyboard navigation, field and deck scope, cross-page results, data isolation, stale revisions, shared fields, cancellation, counts on deletion and successful sync. Exercise a large collection without materializing it in the browser. Check search against sparse fields and misleading HTML or media text. Distinguish browser fixture proofs from real server and native-sync evidence.

## Definition of Done

The implementation satisfies R1–R11 and AE1–AE4, has independent source review, and is committed on the task branch. The PR against master carries focused receipts, hosted checks including similarity, and any inherited gate failures. Integration and deployment are separate actions.
