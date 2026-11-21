# Interrupted Meguri Sync setup media — review log, 2026-11-21

Start: `ccc64d289`. Fixer sessions `s-20261121T060606Z-b56683f6` (worktree `sync-media-resume`, PR #49) and `s-20261121T064626Z-8fee042a` (worktree `sync-pane-layout`, PR #50, stacked on #49). Spec: `sync/meguri-provider#MP4`, `#MP5`, `#MP6.5`, `#MP6.8`; `sync/status-strings` T19.1, T19.2, T19.5, T19.7, T19.8, T20.1; `docs/design/native-interface-contract.md` N15.2, N15.5, N23.1, N23.3. Lines marked "Changed 2026-11-21 (resume fix)", decided by Claude (coordinator) under Antoine's delegation of 2026-11-21 and his permission of 2026-11-20 to update specs.

## Report

During the first Meguri Sync setup the collection had transferred and thousands of media files were still downloading; Antoine quit and relaunched. Settings › Sync then showed "Media Incomplete", "1 sync issue" and "Meguri couldn't confirm which media files transferred", and nothing continued until he found Retry under Troubleshooting. The Troubleshooting page's back button sat under a centred title, and the pane's title moved from leading to centred.

## Causes

1. Not the core: host test `provider_media_kill` kills the process during setup's media step; on reopen the same operation (phase `MediaIncomplete`, media stage `Running`) keeps setup's admission, the activated collection is still live, and the step runs again and completes.
2. The app mapped every retained collection-complete or media-incomplete operation to the stage for listed files that cannot transfer, and no launch path continued it (MP4 had inherited `anki-transfers#M5`'s explicit resume). The accounting is in memory only, hence "couldn't confirm" and a bare "1 sync issue". Its one Retry, an issues action, lived in Troubleshooting.
3. A media run that stopped early in the same session still called `complete`, whose refusal made the stage "setup failed" with no action that could run.
4. Mac header: SwiftUI gives the Settings scene's window the preference toolbar style (captured in the VM before and after).

## Fix

PR #49: paused and stopped media stages; `MeguriMediaResume` (triggers: foreground launch, foreground return, network return, cleared block or hold, sign-in, waits of 30 s and 120 s; three counted attempts; offline and outside stops not counted); refusals classified (unfinished, stopped, blocked only for excluded files, setup's identity failure); one status row with Try Again in the Sync section and the panel. PR #50: the bar in the status row; one sentence with the files left; "Downloading Media…"; the issues row only beside the state (media blocked with no list known is waiting with Try Again); Retry in Affected Files; Service reason naming the media; Mac help tag for Sync Now; the Settings window's unified toolbar style; locale-grouped counts.

## Independent review

Opus reviewer, PR #49, first pass: no P0; one P1 (a step stopped by going to the background stayed on its moving title), three P2 and five P3; all fixed, with a coordinator-level test of the waits and the stop. Second pass: no P0 or P1; one P2 (a read that found setup complete skipped setup's last steps) and five P3; all fixed. Open: the launch, foreground and sign-in triggers have no unit test (no fake bootstrap facade exists), and the `.transfer` refusal branch cannot be built in a unit test.

Opus reviewer, PR #50: no P0. P1 media blocked with no known count had no Retry anywhere (fixed). P2 toolbar style re-applied when reset and recorded as an exception (fixed); P2 tests for view-level helpers partly added (media blocked without a list, catalog, locale). P3s fixed: bar accessibility, stale docs, help-tag scope, bar placement.

## CodeRabbit

PR #49: five comments, all fixed (stopped-to-paused after a failed Try Again read; only excluded files block; Ready read queues the first round; refused starts keep an automatic trigger; Ready adoption retried on failure). Round 2: a failed Ready settings read stays pending for every later trigger, sign-in included; a settings write made during that read wins; a trigger refused during a service change is replayed when it ends. No unit test drives these paths (no fake provider refresh or handoff exists).

PR #50: four comments: the window preview opens at Sync; transferring media has its own T19.5 row; the media-blocked dead end (fixed with the review's P1); Ready adoption (fixed on #49).

## Load note

`selectedBoundHostFixedEditUsesNormalUpdaterAndCommittedRevisionReadback` failed twice while other heavy jobs ran on this Mac; it passed alone on PR #49, on its base `ccc64d289` and on master `12c90b61b`.
