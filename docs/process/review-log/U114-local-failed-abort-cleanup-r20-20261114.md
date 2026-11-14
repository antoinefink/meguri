# U114 local failed-abort cleanup, R20 — 2026-11-14

Starting commit: 88082324. Integration base:
1416a520542a8c5daa38a23bfe398da704c3d6c9.
Authority: sync/cloudkit-recovery V14.3. This extends the R19 repair to local
work; it does not accept the sync unit or release.

Local adoption consumed its work claim before checking authority and running
validation. If mandatory abort then failed, the claim remained active after
the work had ended. Later Cancel and drained Close both returned Busy.

Reply processing and local validation now use one guarded cleanup transition.
The local paths call it only after abort returns an error. Cleanup requires
the same retained owner and a finished claim; uncertain activation is refused.
Account restoration, active-claim refusal, SessionActive handling and error
precedence are unchanged. The transition allows cleanup without task replay.
There is no public API or schema change.

The new tests obtain public local-work handles through the existing staging
flow and use the real worker pool before arming provider faults. One control
corrupts only the synthetic candidate and proves actual validation rejection
with successful abort. Two regressions cover authority-read plus abort failure
and validation-body plus abort failure. They retain typed errors, stage bytes,
tracking row, slot, generation and live collection rows; reject stale handles
and wrong owners; and check another cleanup failure before successful public
Cancel or drained Close. Existing reply and activation tests stay enabled.

Filtered author 20261114T225226Z-99177 produced the candidate. Independent
review 20261114T225755Z-2929 found no P0, P1 or P2 finding. Both integrated
files match the author and independent review byte for byte.

Test-only baseline 20261114T225811Z-3454 passed the validation control and
failed both recovery cases at Busy against unchanged R19 production. With
the repair applied, 20261114T225834Z-3838 passed all three unchanged tests.
Full FFI run 20261114T225914Z-4811 passed all 395 tests with none skipped.
Privacy 20261114T225845Z-4126 passed both files. Similarity
20261114T225846Z-4159 scanned both with no findings; only the safe
implementation report was read.

Rust lint 20261114T230124Z-5576 and static checks
20261114T230123Z-5266 passed all three checks each.

The new fault coverage centers on live-collection Validate. Initial activation
and successor interleavings were reviewed but are not newly exercised here.
Native integration, required batch gates, wider CloudKit lanes, other
platforms, native replay identifier evidence and final acceptance remain open.
