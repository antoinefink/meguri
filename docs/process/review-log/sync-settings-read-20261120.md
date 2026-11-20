# First-run Sync status fix — review log, 2026-11-20

Start: `2c8c10ba5cc49b0d6b4bb98483fc65673529f402`. Fixer session `s-20261120T173419Z-0e6629db`, worktree `sync-settings-read`. Spec: `sync/meguri-provider#MP1`, `#MP4`, `#MP6.5` (new lines under review, see its Status line); `docs/design/native-interface-contract.md` N15.1 (one new line, awaiting approval).

## Report

On the owner's first Mac run with Meguri Sync, the first download's media step ran while Settings › Sync and the toolbar said the device's sync settings could not be read and that Meguri Sync was stopped; the progress area flipped between a spinner and a bar.

## Causes

1. Core's FFI provider snapshot mapped any closed network admission to Blocked. A Meguri setup transfer closes admission until its media ends. Host test: `meguri_setup_transfer_holds_admission_from_preview_until_media_completes`.
2. Same cause; the status rows put the blocked row before the setup stage. No code stopped running work when a real block happened.
3. `SyncSettingsSections.swift` chose between `ProgressView()` and `ProgressView(value:total:)` from the job progress, which alternates per media batch between a request's byte count (with a total) and the media step's report (no total).

## Fix

Core reports the document's phase only when the closure is the selected Meguri connection's active collection-and-media setup (`CoreHost::meguri_setup_transfer_holds_admission`, the reads of `active_meguri_preparation`); admission stays closed. Swift stops a running media step or round when the phase turns blocked, hides the media count and progress then, and draws one linear progress whose media value is files done over files done and left. Copy Debug Info now starts with a summary, adds content-free sync facts (blocked source and failure kind), and prints the export pretty with its timeline last. The Mac Settings panes drop the heading their title bar repeats.

## Independent review

Fresh read-only reviewer, round 1: no P0/P1. Resolved: P2 FFI mapping now tested through `admitted_snapshot`; P3 predicate narrowed to the Meguri setup operation; P3 explicit linear style; P3 spec wording (no Pause reference; preparation left to end; "this run's files"); P3 stop limited to the action that was running. Noted, not changed: `validateRetainedMeguriAttachment` now passes during setup (Core still guards every start); a transient settings-write failure, already reported as blocked, now also stops a running round; the `didSet` stop path has no coordinator-level test (no fake provider exists for it; its decision is tested as `stopsWorkOnBlock`).

## CodeRabbit

Four comments. Fixed: setup continuations now stop while blocked; the media stop identifies its step by facade and stage, so it works on an adopted coordinator. Answered: no routine-round progress bar (unchanged behaviour); the MP6.5 lines are built and await approval before merge.
- Round 2: a block during admission now ends the unconfirmed preparation (Try Again offered, also after a refused or failed discard); a media step a block keeps from starting shows as media blocked with Retry. Untested at unit level: no fake exists for the bootstrap facade or editor admission.
