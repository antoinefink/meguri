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
