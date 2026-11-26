---
title: "fix: One progress layout for the deck import sheet"
type: fix
status: active
date: 2026-11-26
---

# fix: One progress layout for the deck import sheet

## Overview

The deck package import sheet (`settings.importExport`, `package/transfer-sheets` X2.4) looks unfinished while it imports on the Mac. In the final, non-cancellable phase it shows a large spinner floating at the leading edge of a grouped row with "Importing…" beneath it, a separate footer caption ("Finishing. This step can't be cancelled."), and two disabled buttons (Cancel and Import). This plan gives the progress state one consistent, native layout and removes the dead control.

## Problem Frame

Owner screenshot (2026-11-26, macOS): importing a large `.apkg` reaches the Committing stage.

- The indeterminate `ProgressView` with a label renders as a regular-size circular spinner with the label stacked under it, left-aligned in a grouped Form row. It reads as a stray glyph rather than progress, and it differs from the determinate stages (media), which draw a linear bar. The sheet therefore switches between a bar and a spinner as stages change.
- The "can't be cancelled" explanation sits apart from the progress it explains, in the section footer.
- The toolbar shows a disabled Import button although the options are no longer on screen; it offers nothing and adds a second dead control beside the disabled Cancel.

## Requirements Trace

- R1. Every progress stage of the sheet (reading, preparing, media, backup, importing, syncing, uploading) uses the same linear progress layout; indeterminate stages show an indeterminate bar.
- R2. When the work can no longer be cancelled, the explanation appears with the progress itself, under the bar.
- R3. While the progress view replaces the options, the toolbar shows no Import button; Cancel stays, disabled when the work cannot be cancelled (X2.4/I14.5: no Cancel from Committing on), so the button row does not jump between stages.
- R4. The fast path (options still shown while importing, before progress becomes visible) keeps its disabled Import, as today.
- R5. A capture route draws the non-cancellable finishing state so it can be screenshotted and reviewed; its screenshot is attached to the pull request with the GitHub CLI.

## Scope Boundaries

- No change to the import model, stages, cancellation rules or copy semantics.
- No new snapshot-matrix fixture (that would need an approved baseline from Antoine); the new state is a capture route only.
- iOS keeps the same structural change (bar, caption placement, no dead Import); no iOS-specific redesign.

## Context & Research

### Relevant Code and Patterns

- `app/Packages/MeguriKit/Sources/Features/Settings/Collection/DeckImportSheet.swift` — `DeckImportContent.progressSection` and the `.idle, .inspecting, .importing` toolbar case.
- `app/Packages/MeguriKit/Sources/Features/Settings/Collection/DeckImportText.swift` — `progress(_:file:)`, `finishing`.
- `app/Packages/MeguriKit/Sources/Features/Settings/Collection/DeckImportSheetPreview.swift` — named fixtures used by capture routes.
- `app/Meguri/TestHooks/DeckImportRoutes.swift` — `collection/deck-import/<fixture>` routes.
- `docs/design/workflow.md`, `docs/design/native-interface-contract.md` — native controls, inspect loading/busy states.
- UI tests click `settings.deckImport.import` only in the options step (`app/MeguriUITests/CollectionEntryJourneyTests.swift`), so hiding it during visible progress does not affect them.

### Notes from the Repository

- No snapshot baselines exist for `settings.deckImport` (`app/Snapshots/*`), so the `.loading` matrix cell changing does not break an approved baseline.

## Key Technical Decisions

- Linear bar for indeterminate stages: a sheet that reports a long job uses a bar on macOS (Finder copy, Installer); one style across stages removes the spinner/bar switch.
- Caption as the progress view's current-value label: it sits directly under the bar in the same row, like Finder's "Estimating time remaining…". The media stage's file count keeps that slot; Media is always cancellable, so the two never collide.
- Keep a disabled Cancel rather than hiding it: it keeps the sheet's height and button row stable when the stage turns non-cancellable, and the caption says why.

## Implementation Units

- [ ] **Unit 1: Progress layout and toolbar**

**Goal:** One linear progress layout with the non-cancellable caption under the bar; no dead Import during visible progress.

**Requirements:** R1, R2, R3, R4

**Dependencies:** None

**Files:**
- Modify: `app/Packages/MeguriKit/Sources/Features/Settings/Collection/DeckImportSheet.swift`

**Approach:**
- Indeterminate branch: a linear-style `ProgressView` with no value, same label as today.
- Current-value label: the media detail when present, otherwise `DeckImportText.finishing` when busy and not cancellable; drop the footer.
- Accessibility value keeps the counts, and says the step can't be cancelled when applicable.
- Toolbar: show the disabled Import only when `!presentation.showsProgress`.

**Test scenarios:**
- Test expectation: none -- view-layout change in a package with no Settings test target; verified by capture-route screenshots and the existing `DeckImportUITests`/journey tests.

**Verification:**
- `tools/mg build-app --platform mac` builds with zero warnings; `tools/mg lint-ui` passes.

- [ ] **Unit 2: Finishing capture route and screenshot**

**Goal:** A reviewable render of the state from the owner's screenshot.

**Requirements:** R5

**Dependencies:** Unit 1

**Files:**
- Modify: `app/Packages/MeguriKit/Sources/Features/Settings/Collection/DeckImportSheetPreview.swift`
- Modify: `app/Meguri/TestHooks/DeckImportRoutes.swift`

**Approach:**
- Add fixture `importing-finishing`: importing step, progress visible, Committing stage, no total, not cancellable, a long `.apkg` file name.
- Register `collection/deck-import/importing-finishing`.
- Capture it (and `importing`) with `tools/mg screenshot --platform mac --route …`; attach the image to the pull request with `gh` per the GitHub CLI attachments docs.

**Test scenarios:**
- Test expectation: none -- fixture/route only; `tools/mg xcode-check`'s hidden launch of every route covers it in CI-equivalent runs.

**Verification:**
- The screenshot shows title and subtitle, a full-width indeterminate bar with "Importing…" above and the caption below, and only a disabled Cancel.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Mac screenshot needs Screen Recording / a host session | Fall back to `tools/mg snapshots --platform mac --record` in the VM for the route's view; report if neither is available |
| Design agreement belongs to Antoine | The PR presents the screenshot for his review; no baseline or decision is recorded by the agent |

## Sources & References

- Owner screenshot, 2026-11-26.
- `specs/package/transfer-sheets.md` X2.4; `docs/design/workflow.md`.
- https://docs.github.com/en/github-cli/github-cli/attaching-files-with-github-cli
