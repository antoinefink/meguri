# Original-resource job prerequisites

Original prepared start: `783efd28`. Resumed start: `e1f027dc`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Filtered fixer session: `s-20261103T133750Z-f276f58e`.
Contract: `engine/jobs#J2.1.1`; lifecycle and suspend prerequisites at `87a08ead`.
Anki: none. This component does not accept U113, U114 or U151.

Registration records original Host, collection and generation facts under the registry lock.
Full calls finish before resource teardown. DisposalOnly calls retire their retained handles.
Every teardown checks current jobs and busy workers. Successful open or resume alone resets admission.
The Host mirror follows canonical state transitions, including failed-close restoration.
Production asset consumers remain a later integration step.

Independent review found two issues: repeated teardown trusted an old cutoff, and open reset admission before installation succeeded.
Both were repaired. The final prerequisite review found no P0–P2 findings.
Round 4 also reviewed the exact-once fixture correction and found no P0–P2 findings.

Core `20261109T161857Z-242` failed compilation on untyped new-test channels.
After precise unit annotations, `20261109T162652Z-4047` compiled and passed 32 of 33 focused checks.
Its failure counted an unrelated blocker terminal as a duplicate.
The corrected fixture collects through close and checks each of five target outcomes exactly once.
Core `20261109T163000Z-5982` passes that test; the other 32 checks are unchanged.
Assertions, cancellation checks and deadlines remain intact.

Final export `20261109T163036Z-6351` removes only a flagged explanatory comment.
Its other bytes match the reviewed, tested source. Prior failed checks remain retained.
Final lint `20261109T164124Z-19389` passes all four checks.
Privacy `20261109T164124Z-19568` passes 17 scoped files.
Similarity `20261109T164124Z-19398` passes 16 source files, with one excluded and no allowlist use.
The original batch gate and production wrapper proofs remain pending.

Lint `20261109T163201Z-7393` passed three checks and rejected one nested conditional in the new test helper.
Filtered export `20261109T163911Z-17514` uses an equivalent let-chain.
Independent review confirms the same predicate, running-map assertion and return.
No production behavior or deadline changed. The final lint result above covers this equivalent repair.
