# U113 incoming collection and media work

Status: enabled tests-first contracts; incoming execution remains incomplete.
Unit start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Phase start: `781f55522c6dc3070a640520d729d11927243f80`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Producer `cleanroom/20261029T084857Z-34440` prepares a local fixed-name media
write through the existing M6 journal, plus tests for that write and incoming
execution. The actual adoption, transfer and intent executor are not built.
Independent R1 clears the narrow service/driver seam and the tests. Root
extracts all tests and support before applying production.

The tests cover exact replacement, identical content, actual I/O rollback and
a stale service; a subprocess write/reopen; all four Q11 media move/rewrite
replay variants; and adoption validation, crash, account cancellation, safety
backup and media. Their Point gates and typed intent/inbox APIs remain missing,
not scripted or treated as successful.

The original publication target has two approved assertion-type repairs:
PendingDecisionReason::Restore is the actual R6.7 value. Both assertions still
require exactly Restore.

Independent diagnosis also finds an invalid original resolution fixture. It
adopted an acknowledged file and then changed the same name on both devices,
which K15.9 sends to trash. Fixer `cleanroom/20261029T090618Z-41848` keeps that
setup for the trash case. The resolution case instead adopts before either
file is created, checks that the actual file and MediaName entity are absent,
then creates the two distinct contents. Independent R2 confirms that all later
holds, failures, intent ordering, scan, restart and publication assertions stay
unchanged. No production conflict rule is changed to fit a test.

Verification before production:

- `core-test/20261029T090506Z-40904`: expected compile RED for write_exact,
  adoption, intent/inbox APIs and real Point gates. No executed pass is claimed.
- `core-format/20261029T091034Z-43602`: final tests pass formatting.
- `private/20261029T091050Z-43946`: all six test/support files pass.
- `similarity/20261029T091052Z-44194`: six files, no findings or exemptions.

The four P24 cases are enabled contracts, not verified recovery. Their items
remain open. No Unit, P24, delivery or integration acceptance. Next implement
actual Runtime-owned staging/replacement, then nonempty media and intent replay;
reuse the working publication and M6 services.

## Fixed-name local write dependency

After the tests-first commit, root applies only the two-file production seam.
MediaService uses the existing admitted M6 operation and journal, and emits
MediaArrived after success. The child delegates PutMedia to that service.
This local-ingestion API is not the K9.3 incoming install or case-collision rule.

- `core-format/20261029T091150Z-44750`: pass.
- `core-test/20261029T091221Z-45707`: 69 engine library and media cases pass,
  none skipped. The four new exact-write cases and existing process-crash,
  journal, publication, rollback and generation checks execute.
- `private/20261029T091219Z-45231`: both production files pass.
- `similarity/20261029T091220Z-45469`: two files, no findings or exemptions.

The new child media-write target also shares adoption helpers, so its execution
still awaits that real API. Adoption, all four replay cases and the original
full-workflow target remain enabled RED. No lint, gate or P24 pass is claimed.
Next build the Runtime-owned adoption path against these committed contracts,
then finish real nonempty-media transfer and replay. Do not add successful
stubs or replace the existing verified publication path.

## Collection-adoption implementation, tests first

Producer `cleanroom/20261029T091828Z-48231`, based on `402a9b1c`, exports an
actual Runtime/CoreHost adoption path and one separate focused target. Root
extracts that target before production. Independent review clears its declared
empty-physical-media slice: an actual typed Staging row, real source FetchZone,
remote rows and crt, source keys without new draws, a new publication identity,
real CloudAdopt evidence and safety package, and persistence after reopen.
It does not accept full incoming media or overall review usability.

- `core-format/20261029T094605Z-56718`: pass.
- `core-test/20261029T094723Z-57342`: expected compile RED on missing adoption,
  typed facts and real staging gate before production. No executed pass claimed.
- `private/20261029T094724Z-57565`: one test file passes.
- `similarity/20261029T094724Z-57576`: one file, no findings or exemptions.

The 18-file production export remains unapplied and under independent R1.
Known unfinished conditions include nonempty media/feed, assets, tombstones,
structural generations, previous/initial/damaged adoption and ambiguous R6
startup cancellation. The new positive slice supplies no waiver for those.

## Adoption guard repairs, tests first

Production R1 identifies two P1 defects: activation recovery could replace its
recorded native authority with a different current file, and staging did not
compare note field count with its notetype. Fixer `095549Z-61045` adds typed R6
activation proof and the field-layout check. The new host guard target uses an
actual post-activation abort, a cloud-disabled CoreHost journal drain, then a
byte-identical new-native file. It requires successful Runtime admission before
checking isolation and retained evidence. Recovery of the original file is also
tested. Actual Service writes cover field-count errors and valid empty fields.

R2 clears those repairs and identifies two more P1 defects: closing an unowned
retained recovery incorrectly attempted cancellation, and a valid R6 account
mismatch decision (Clear to Pending) was classified as FileMismatch. Fixer
`101853Z-74330` closes only live owned work and accepts only the exact proven
pending variation, preserving Pending before normal account isolation. It adds
real account-change, retained-evidence close/reopen, and live-slot cleanup cases.
Independent R3 finds no remaining P0-P1 or new P2 in this repair.

Root extracts the combined host target before production. Cancel refusal for
retained uncertain recovery is safety coverage, not implementation of the final
ambiguous-recovery Cancel requirement.

- `core-format/20261029T102851Z-78841`: pass.
- `core-test/20261029T102940Z-79925`: expected pre-production compile RED on
  missing adoption and test IPC APIs. No executed pass is claimed.
- `private/20261029T102943Z-80199`: flags one comment and a custom test name.
  Fixer `103119Z-81450` removes three comment lines and renames that test only;
  independent equivalence review confirms all assertions and inputs unchanged.
- `private/20261029T103320Z-82960`: final test file passes.
- `similarity/20261029T103316Z-82691`: one file, no findings or exemptions.

Compilation and focused execution of the 19-file production repair follow this
tests-first commit. Full incoming media and the previously listed requirements
remain open; no Unit, P24 or integration acceptance.
