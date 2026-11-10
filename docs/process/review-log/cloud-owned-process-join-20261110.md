# Owned setup process cleanup

Base: `5267339c`. Infrastructure repair; no unit completion or signed acceptance.
Antoine requested that this repair be completed directly. The prepared
process wrapper was retained and reviewed locally before integration.

## Failure and change

MG `20261110T194812Z-84309` reproduced a transport EOF followed by EPERM in
cleanup. The original child had exited but had not been reaped. That second
error hid the first one. Earlier probes had observed the child before closing
it and did not cover this case.

The managed original keeps its PID reserved through nonreaping exit observation
and a complete check that its group contains only that terminal original.
It then calls waitpid once and checks the actual result. Poll, wait and
communicate cannot report completion before those checks. Cleanup never signals
a reaped PID. Foreign members, incomplete observations, lost child ownership
and mismatched results fail closed. A cleanup failure remains a technical
failure; the original request error is retained as its cause.

The existing command, transport and shutdown budgets are unchanged. Enumeration
is limited to the original reserved group. This does not claim to find escaped
processes in other groups. No account values or SDK records are inspected by
the regression test.

## Review and checks

Local review corrected two issues in the prepared draft: final reaping must
use an explicit checked waitpid, and uncertain cleanup must not preserve a
setup requirement that could hide the cleanup failure. A failed group check
also prevents any later signal or completion claim.

MG `20261110T202705Z-92208`: seven focused tooling wrappers passed, including
the generated ownership models, transport lifecycle and owner setup tests.
MG `20261110T202843Z-93213` ran 32 related tooling wrappers: 31 passed, and
one retained an old launcher mock. That mock was moved to the managed factory.
MG `20261110T203052Z-95121` then passed all five affected wrappers, including
the added cancellation tests and selected-coordinator coverage.

Privacy scan `20261110T203048Z-94857` passed. The guard remained unchanged.
Similarity scan `20261110T202856Z-93600` passed before field renaming; the
final source scan `20261110T203120Z-95750` passed all eight files.

MG `20261110T202748Z-92554`: the same native VM EOF reproducer now passes.
There is no poll, wait or status observation between the original EOF and
actual owner close. Close succeeds and the original exit23 is retained.
No account lookup, SDK work, app launch or native build occurs in this probe.
The temporary MG probe route was removed afterward.

The final integrated patch has no new independent review.
Antoine requested direct work. Full integration and real signed setup
remain pending. Passing cleanup is not proof of a working CloudKit round trip.

## Signed setup follow-up

Commit `050e260c` was pushed before owner setup `20261110T203215Z-96644`.
The run still fails, but cleanup now joins and relays the original guest's
closed diagnostic: `guest-app-receive / transport-frame`. The original stream
and stage were checked unchanged by classifier `20261110T203404Z-99438`.
This means the response ended before a complete frame arrived. It does not
prove that the app sent malformed data or that an account is missing.
No approval was entered and no source was published.

Local follow-up review found that parent cancellation during a nonreaping
observation must leave the original available for cleanup. It is not an
ownership failure. The narrow correction retains the failure fence for all
other observation errors. Focused run `20261110T203513Z-99768` passed all three
affected wrappers, including the new cancellation case.

The setup-only Swift test now records only enumerated operation and channel
checkpoints on refusal. The guest relays only bounded, exact-schema marker
records after its original child joins. Host relay retains the same boundary.
Raw XCTest text, paths, exception descriptions and account values stay private.
The directory, socket, event and loaded-product checks remain enabled and
unchanged. Native compilation and the next setup attempt are still pending.

Setup `20261110T203853Z-2605` compiled and reached the app test. It failed
before the account read at `observationChannel / attributeEvent`, relayed by
classifier `20261110T204033Z-6258`. No approval or publication occurred.
The diagnostic now distinguishes only three directory roles: the private
stage, the OS support root and its ancestors. It exports no directory name.
Focused check `20261110T204532Z-8995`, privacy `20261110T204532Z-8978` and
similarity `20261110T204533Z-9460` pass for that refinement.

Generated VM probe `20261110T204420Z-8505` measured the existing vnode mask.
Directory and file creation did not yield an attribute event. Setting the
same mode, changing and restoring permissions, changing and restoring an
extended attribute, and touching timestamps did. Thus that notification alone
cannot establish the cause. Earlier probe failures were fixture diagnostics:
the VM's Python lacks os.setxattr; the corrected probe uses the public Darwin
interface. No guard was removed or relaxed, and this probe makes no account
lookup or CloudKit call.

## Owner channel ancestor metadata

Base: `16eced51`. Setup `20261110T204623Z-9988` again failed before account
read. Classifier `20261110T204757Z-12694` identified `ancestorAttribute`, not a
private-stage or socket change. All current directory and socket pins had
passed before that notification was consumed.

The owner channel is now shared test support so generated real-socket tests
exercise the exact class used by the app-hosted setup. Ancestor device, inode,
owner and mode bindings are still checked at every boundary. Rename, delete
and revoke notifications remain active for the whole chain. Attribute-event
history remains required on the private setup directory; it is no longer a
history requirement for OS-managed ancestors. Thus the channel does not claim
to detect a permission change and restoration on an ancestor between checks.
The host's approved-source admission and private input checks are unchanged.
This changes no product account fence, database operation or approval rule.

Six tests pass in `swift-test/20261110T205021Z-13032`: benign OS-ancestor mode
and timestamp updates; current ancestor permissions; private permission
change-and-restore; private timestamp events; ancestor rename-and-restore;
and replacing a socket with another socket of the same mode. A failed channel
check is now retained, so another check cannot clear the original refusal.

Focused setup tooling `20261110T205044Z-14680`, privacy
`20261110T205043Z-14437` and similarity `20261110T205044Z-14871` pass. These
are generated tests with no account lookup. The next signed setup run and
full batch integration remain pending; there is no round-trip acceptance.
