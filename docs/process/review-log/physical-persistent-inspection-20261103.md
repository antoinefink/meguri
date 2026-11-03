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

Physical run93588 builds and verifies the signed development products. Its
private install witness records successful app/runner installation and matching
metadata. A later public device query fails before preparation completes; no
continuity evidence is produced. The generic query refusal is retained.

Export96382 retains bounded private failure diagnostics before query scratch
cleanup. Independent review confirms unchanged query scope, count, timeout
and original refusal, with no public raw values. Selftest98631 passes the
affected metadata/diagnostic seam. Private99042 and Similarity99310 pass.
The next physical observation must identify the underlying error; no device
setup or successful launch is inferred from the generic refusal.

Run99663 retains fixed context: normal-app-launch, command failure. Neither
requested output file exists. Public CLI help shows that launch has a variadic
app-argument tail. Independent fact-checking supports placing device/output
options before that tail; it does not prove why the earlier invocation failed.
Export2395 makes that narrow ordering correction for all four supported verbs.
Independent review clears it without guard or diagnostic changes. Selftest3119,
Private3390 and Similarity3680 pass. A new physical run must still establish
whether launch succeeds and whether the process survives later inspection.

Normal run4020 passes signed preparation, installation and launch with the
corrected option order. Preserve drive5415 then refuses before starting its
runner: Xcode's trusted generated plist contains three target metadata keys.
It supplies no target arguments. No app restart occurs in that refusal path.

Export5948 accepts only those known string-valued metadata keys and strips
the whole target environment and empty argument field from the destination
copy. Unknown keys, malformed values and nonempty arguments remain refused;
host environments are preserved. Independent review clears the change.
Selftest6642, Private6943 and Similarity7210 pass. The app/core products and
their source digest are unchanged, so inspection reuses run4020 directly.

Actual drive7555 passes both launch and snapshot steps with zero issues.
Its completion evidence reports requested retention, activation of the existing
app and verified continuity across the entire invocation. No build or install
is performed. The exact render shows the normal No Collection screen, so this
is app/process evidence, not a completed import or CloudKit round trip.

The next retained session opens the connection preview through the existing
home action. Repeated inspection, failed-step retention and default termination
still need their remaining physical coverage; synthetic receipts alone do not
satisfy those requirements.
