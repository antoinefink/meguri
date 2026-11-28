---
title: Filtered decks and Custom Study
type: feat
date: 2026-11-26
artifact_contract: ce-unified-plan/v1
---

# Filtered decks and Custom Study

## Goal

Let someone study a selection of existing cards, either by writing a Browse search or by choosing a Custom Study preset. Their cards still belong to the original decks. Today's limit extensions apply to ordinary study.

## Product contract

The user's request supersedes the original filtered-deck exclusion. The behavior contract is `specs/decks/filtered-study.md` F1–F6. Keep the existing Anki-compatible deck and card records and Meguri Sync transport. A preview must preserve the card's home schedule across a repeat, relaunch, Empty and return. A scheduled answer uses the home preset. A normal deck cannot be overwritten by a similarly named study session.

## Implementation

### U1. Filtered deck management and custom study

Apply F1, F2 and F5 in the collection writer. Search against the transaction after Empty so both terms see the same card membership. Test overlapping terms and consecutive rebuilds at the limit.

Edit an imported definition and reopen it. Check the legacy preview delay and unknown fields on each search term. Exercise a day-zero card through Empty, Delete, preview completion and Undo. Expose these operations through the existing UniFFI collection service.

### U2. Study the selected deck

Extend the existing queue, counts, scheduling states, answer transaction and review session to handle filtered membership. Use stored positions for selection and saved due dates for scheduling. Cover new, learning, early and due review, timed preview repeat and completion. Exclude these cards from ordinary study while borrowed. Reject a stale answer if Empty or Rebuild changed the presented head. Exercise Undo and check card fields and history before and after return.

### U3. Native entry points and evidence

Use native forms for a named filtered deck and a deck-scoped Custom Study session. Make Study primary on the filtered deck page and expose Edit, Rebuild and Empty in context. Launch Custom Study from a normal deck's menu or finished deck page. The finished review keeps its summary and Done, returning to that deck page (owner, 2026-11-28; F6). Allow creation from Browse's current search. Make cancellation inert and surface writer failures with the draft intact. Refresh deck selection and counts after mutations.

Verify on synthetic cards with focused MG Rust and Swift tests. Drive creation and Custom Study through the native Mac VM UI using the project's test hooks, attach window screenshots for both tasks and verify the uploaded links on the PR. Run affected Apple builds, independent review and the repository's provenance, privacy and similarity checks. Record any inherited failures separately.

## Sources

- `specs/model/protobuf-records.md` P6.5
- `specs/decks/decks.md` D9
- `specs/sched/presets.md` V1 and D4
- `docs/design/native-interface-contract.md` and `docs/design/workflow.md`
