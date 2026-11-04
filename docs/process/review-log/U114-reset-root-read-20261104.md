# U114 reset root-read tests

Status: WIP. No unit acceptance or P24 closure.
Unit start: 218ab2e2543a2c4822b79c1ac3520230ca3c1998.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Test component start: 399e082a19d5d8a24269d6d5ec0d5b3fd06422cb.
Fixer: s-20261103T133750Z-f276f58e.

The reviewed contract clarification preserves the existing missing-root case.
A first root read that reports unknownItem permits a conditional new-root save.
Other failed first reads do not permit a save or deletion. The second-read
rules and guarded deletion requirements remain unchanged.

Export66369 adds a fixture path through a real reset-bearing cleanup reply.
It preserves the original request, guard and collection. Independent review
found no P0–P2 issue. Format82428 passed. Core83516 failed compilation because
the two digest libraries require separate trait imports. Export84649 adds
only that missing import.

Core89607 then compiled and ran four tests. The reset-authority and persistence
test passes. The test characterizing the current refusal also passes. The
readable-root and missing-root tests both fail at the exact
ResetRecoveryUnavailable guard, before a root read. This is the intended red
baseline for missing behavior, not recovery acceptance.

Similarity90058 passes. Private89883 flags two new test lines. That repair and
stronger native error-outcome coverage remain in progress. The original
classification-only 37-code test does not close Q11. Production reset behavior
is unchanged. Destructive recovery, its epoch proof and timing checks remain
outstanding.

## Native error mapping

Export97071 preserves the original generic read tests and adds publication
reads, terminal-only partial errors, unmatched record IDs and precedence cases.
An independently reviewed callback-capture seam exposes the original installed
handlers. It changes no production callback behavior.

With only that seam and the tests, Swift2812 fails four expectations: terminal
errors for codes 11 and 26 become generic failure in both read modes. The
decoder fix uses only the requested record's result in the terminal dictionary.
Per-record callbacks retain precedence. Swift4229 then passes all 22 native
transport tests, including the 37-code matrix. Independent review reports no
P0–P2 findings in this bounded change. This is native mapping evidence, not
proof of the full reset workflow or signed cross-device convergence.
Private5415 and similarity5416 pass.

## Bounded publication recovery

Core component start: 39e2fca9. Export97071 adds the first reset-root read
after zone creation. A readable root supplies its existing media key. Only a
missing-root result permits a conditional save. Failed, zone-missing,
malformed, newer-format and absent-e results remain pending without a media
root save or deletion.

Permission to save retains the original request and guard, is consumed before
submission, and is not persisted. Binding changes, invalidation and reopen
clear it. Reopen must read again. No storage schema or FFI signature changes.

Core6235 records the pre-implementation result: one authority test passes and
nine required workflows fail at the old guard. Core7180 then passes 40 of 41
tests across publication, cleanup, intent replay, submission and adoption.
The remaining assertion counted both the publication read and a normal media
startup read. Independent review traced both owners. Export9243 requires
exactly two reads, one per owner, in order; its first compile10640 failed type
inference. Export11914 changes only collection syntax. Core13341 passes the
corrected test, including clean close and process-kill variants. All other
assertions and production bytes remain unchanged from Core7180.

Final independent review reports no P0–P2 findings. Core lint14792, private14216
and similarity14217 pass. Swift15665 passes 18 connection bridge tests against
the rebuilt core. The earlier native suite4229 passes 22 tests.

The full batch gate remains pending. P24/Q11 remains open for the complete
bridge witness and destructive reset protocol, including the own-epoch proof,
second read and deletion timing. Adoption after a reset is unchanged. No unit
acceptance or signed cross-device convergence is claimed.
