# U114 R11 account proof

Status: WIP; no unit acceptance.
Unit start:218ab2e2543a2c4822b79c1ac3520230ca3c1998.
Component start:e20b54649c2a35a22b4c347baf1b567e54a0c41e.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T133750Z-f276f58e.

Antoine approved correcting the test to the existing contract. R11 changes
the test only. Matching account proof accepted in the current process can be
reused; a later account read need not acknowledge again before Resume starts.
Fresh-process proof, exact final SDK count, maximum live count, command and
ACK ownership, cleanup joins and timing limits remain required.

Candidate51301 passes Private52438 and Similarity52702. Independent source
review initially clears it. Swift53033 then fails the Resume precondition and
its dependent SDK count assertion. The receipt remains failed.

Further independent review identifies the mistaken witness. A stopped binding
is unusable, so `accountMatchesBinding` can be false after an accepted matching
account read. It does not represent the current-process identity proof alone.
The ready ConfirmResume preview on the same backend supplies the relevant
binding/account authority witness. Capture it before Resume clears the preview
and combine it with accepted reads and available account status. No runtime
verification guard or ordering change is proposed.

Candidate51301 is unapplied intact for a clean filtered-runner baseline.
Its patch and failed check evidence remain retained.

Corrected export56181 captures the ready preview before it is consumed and
ties it to the same backend's accepted account reads and available status.
Two independent source reviews find no P0-P2 issues. Swift56895 passes the
focused test, including both cases. Swift58639 passes the retained selection:
17 tests, zero failures or skips. Private58964 and Similarity59950 pass the
corrected source; the documentation scan is recorded with this commit.

This closes the retained Resume test failure under the approved contract.
It supplies neither a full gate nor a live CloudKit round trip. U114 remains
unaccepted pending the other required verification and integration evidence.

## Account test evidence

Export64090: source56828 plus compiler repair. Swift62096: build failure.
Swift65594: three tests; two pass, one fails four expected assertions.
Outputs, ACKs and cleanup pass. No gate or unit acceptance.
