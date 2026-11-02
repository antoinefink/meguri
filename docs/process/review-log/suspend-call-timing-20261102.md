# Suspend call timing regression

Start: a15dd42b34e624371be4efe4e96e151c3340669a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261025T162405Z-9dc656e4.
Prepared export: cleanroom/20261102T082858Z-5892.
Independent reviewer: native_bounds_review; no required finding.

The original full gate failed control_lane_contention_counts_toward_suspend_deadline
at suspend.rs:373. Its stopwatch included thread creation and joining. The
correction measures the worker's actual suspend call. The caller still holds
the control lane until join; the 20 ms deadline, exact SuspendDeadline error
and 200 ms upper bound remain unchanged. Production code does not change.

Focused core-test/20261102T115332Z-13429 passed the one selected regression;
4,378 tests were outside that selection. This does not replace a full Rust
check or the original failed integration gate. Native builds were idle when
the timing assertion executed. Separate Swift compilation cleanup changed
no Rust input while this focused job ran.

core-format/20261102T115734Z-16825 passed. Privacy and later batch
integration evidence remain separately required. No Unit trailer or gate acceptance is claimed.
