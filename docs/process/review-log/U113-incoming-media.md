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
