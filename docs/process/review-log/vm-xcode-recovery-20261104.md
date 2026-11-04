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
