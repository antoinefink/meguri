# Exact native testkit diagnostic selection

Starting commit: `8919090c`. Integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The existing native conformance runner can now select one complete Cargo-reported
testkit test binary with `--lane native --native-test <name>`. It rejects unknown,
ambiguous or invalid executables; other lanes, live mode and scenario filters
cannot be combined with it. It retains scratch ownership, whole-binary execution,
nonempty test counts and the original concurrency limit. Default conformance
coverage is unchanged.

Receipts explicitly identify this as diagnostic-only. The distinct arguments
cannot satisfy full conformance, gate or milestone requirements. This permits
measuring the retained native Reconcile timeout without running unrelated suites.
No timeout, assertion or retry behavior changes.

Filtered author: `cleanroom/20261115T000223Z-29198`.
Independent review: `20261115T000817Z-31612`, no P0–P2 findings.
Focused `selftest/20261115T011516Z-2920`: all five passed, zero failures or skips.
This covers exact selection, refusal/results validation and unchanged default
native scratch behavior through controlled runners; it does not prove a native
application or conformance pass. Actual timeout measurement remains to run.
The full integration gate and unit acceptance remain outstanding.
