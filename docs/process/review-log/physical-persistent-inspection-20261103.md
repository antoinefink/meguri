# Physical-device harness review evidence

Status: WIP; physical continuity remains unproved.
Starting commit:e20b54649c2a35a22b4c347baf1b567e54a0c41e.
Final candidate base:ef38a1b7fac6f98ee98a5706dc436d3677d26d03.
Session:s-20261103T150638Z-42b1ad5b.
Contract:tools/drive-scripts/physical-device.md D1-D4.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.

Teardown of short UI tests interrupted the first adoption attempt. Device
preparation can install the development build; an opt-in drive activates it
and keeps it running beyond XCTest. Other drives still clean up as before.
Only the harness changes; sync behavior and existing time limits stay intact.

R1 export49888 refuses preservation without executing a positive path.
Independent review confirms that its practical evidence can use the selected
process, executable and installation identities. A trusted successful install
of the locally verified sealed runner binds its nested test bundle; no invented
process nonce or separate plugin inventory is required.

R2 export55677 supplies the destination-artifact runner and completion checks.
Independent review finds one P2: failed completion-manifest publication can
leave a passing status. R3 export71110 explicitly handles parsing, merging,
rename and detail-export failures, retaining earlier failures and counts.
Independent review clears that correction with no new P0-P2 findings.

Selftest77841 passes all 17 focused physical transport and preservation tests.
It covers positive execution, refusal paths, failed steps, private artifacts,
continuity evidence and publication errors. These are synthetic seam tests;
they do not prove installation or process survival on a physical device.

Private78920 flags prose and a signature helper region. Export79804 reworks
that region with equivalent expressions. Independent semantic review confirms
the same checks, command order, errors and public fields. Private82350 passes.
Selftest82676 passes the affected metadata/continuity test after the rewrite;
the earlier 17-test receipt retains coverage of the unchanged transport paths.
Similarity84500 passes the complete component changes from e20b5464.

The actual phone preparation and continuity checks, and the required integration
gate remain pending. A live iCloud round trip is still incomplete. No unit
acceptance is recorded here.
