# VM Xcode recovery evidence

Status: WIP; no unit acceptance or app gate.
Start: bfbab506. Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

The recovery utility admits only run94582, whose first read-only query failed
before preparation. It checks retained evidence and active work, stops the
dedicated VM, confirms that stop, writes separate proof and archives the whole
lease without replacement. It never starts the VM. Original evidence remains.
Target identity uses the configured VM name under root control; it does not
prove a historical VM identity.

Independent review required corrections to host process ownership checks and
child lifetime. Export18461 addresses them. The current staging helper had the
same child lifetime defect; export26790 repairs it. Normal completion sends no
signals. Cleanup retains the original child until its last possible signal.

Selftest27426 fails two harness checks. Diagnostic27735 identifies an archive
attempt mistaken for success and a shell loop retaining its expected failure
status. Export37392 corrects those checks. Original safety assertions remain.
Privacy failures28110,39166,43804 required text and test identifier changes.
Exports40652 and45532 preserve behavior; independent review is clear.

Selftest48651: 15 pass. Private48637 passes. No VM operation has run yet.
These focused checks do not provide staging, signed execution or app acceptance.

Inspect50038 refuses before any stop or lease change. Admission counted 11,791
unrelated temporary entries, read a contextless folder and selected seven old
contexts by lease instead of the exact run. The original context still matches.
Export56656 limits discovery to 64 relevant names within five seconds, selects
the exact run and keeps the full binding and stability checks. Separate activity
proof remains required. Export63105 changes one internal name; review is clear.
Selftest65675: three pass. Private65597 and similarity66085 pass.

Inspect67021 still refuses without a VM change. Export67742 adds only a private
failure location trace: fixed classes, bounded numbers and helper frames, with
no exception text or paths. Review is clear. Selftest83818: three pass.
Private83562 and similarity83966 pass. Actual recovery remains unverified.

Inspect84573 identifies the exact refusal: admission expects a run field in
four valid source-only contexts. MG creates that format without any VM work.
Export87320 accepts only its exact self-bound schema as a noncandidate; pins
and activity checks remain required. Unknown formats still refuse. Review is
clear. Selftest92871: three pass. Private92662 and similarity92435 pass.

Inspect97198 reaches the process reader. Full BSD metadata is protected for
some system processes. A separate numeric-only host probe reads 260 short
records and identifies them as foreign. Exports2542 and10030 use that public
ownership record before full reads. Relevant and pinned lifetimes still need
full checks; unknown ownership still refuses. Review is clear.
Selftest16399: three pass. Private16661 and similarity16157 pass.

Inspect17478 rejects a new owned process before classifying its activity.
Export28212 captures its full identity and arguments under the existing cap;
managed work still refuses through the same activity check. Pinned checks stay
required. Independent review is clear. Export35730 only removes a comment.
Selftest37394: three pass. Private37305 and similarity37332 pass.

Inspect39835 passes with no active work. Recovery40356 refuses before the
stop intent: a read-only lookup changes Tart's Accessed field. No stop ran.
Root verified the joined worker and pre-stop trace, recorded separate proof,
and archived only that attempt's advisory claim under the existing mutex.
The original fence, holder and failed receipts remain intact.

Exports49532 and57832 exclude only Accessed, Size and Running from namespace
comparison. Expected state stays required; present Running must be Boolean
and agree with it. Other fields stay strict and full metadata stays private.
Independent review is clear. Selftest60040: three pass. Private60349 and
similarity60568 pass. Actual recovery and app acceptance remain unverified.
