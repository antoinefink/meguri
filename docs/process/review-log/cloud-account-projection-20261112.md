# Account projection evidence — 2026-11-12

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Start: `563c7d0b`. Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
No unit, milestone or signed-lane acceptance is recorded.

Commit `703feaf3` adds Debug-only error provenance at four account-projection awaits. The result holds a finite classified value, never the error or its payload. The existing recorder retains its limit. Release behavior, ACKs, account authority and cancellation remain unchanged.

Filtered author `cleanroom/20261112T002658Z-51230` supplied the patch. Independent review `003042Z-53491` found no new P0–P2 issue. The privacy hook rejected two new test lines. Filtered rewrite `003153Z-54015` replaced the complete test method, preserving its checks. Final exact-diff review `003423Z-54580` found no new P0–P2 issue.

## Focused verification

- Baseline `swift-test/20261112T002908Z-51757`: the retained original-result resume test passes both collection and media variants.
- Core receipt `003448Z-55153`: 42 tests pass for the curated diagnostic and native trace. The original error is not retained. The previous log cap remains in force.
- Kit `003459Z-55152`: both resume variants pass with the real Service and original SDK result held.
- Static checks `003451Z-56152` and similarity `003452Z-56151` pass.
- Normal signed phone preparation `run/20261112T003607Z-58548` passes.

These checks do not replace the pending integration gate.

## Native comparison

No-change foreground drive `003815Z-59873`: 15 actions pass, continuity verified; 144 hierarchy findings keep it failed. Later read-only observation `004047Z-61082`: three actions pass, 36 hierarchy findings. No connection-tracking fault appears. The startup cancellation remains the latest classified failure. No incoming record was expected from this comparison.

A fresh synthetic deck rename on the exact signed Mac preview then produces an original Saved record. Its process total advances from one to two, with no save failure and zero pending items. No phone restart or preparation follows.

Incoming-change foreground drive `004708Z-61987`: 14 actions pass, continuity verified; 153 hierarchy findings remain. Its immediate snapshots precede delivery. Later read-only drive `004848Z-63169`: six actions pass, continuity verified; 45 hierarchy findings remain. The fresh name is visible. Original native modifications and successful Core forwards both advance from zero to one. Pending, in-flight and waiting incoming counts are zero. The counted trace advances from 30 to 49. No new request, foreground cycle or restart occurs in that observation.

This proves arrival and application of that fresh change. It does not reproduce the earlier tracking fault, prove reliable cold discovery, or clear accessibility failures. The original missing media, complete review equality, rendered media, zone-loss recovery and full integration evidence remain open. Private captures and account values are excluded.
