# Review media test isolation

Status: focused checks pass; original VM Unit failure remains open.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Source start: f6e0b036; integration start: d4ffce06.
Implementer: s-20261101T235351Z-264d9cd0.
Filtered export: cleanroom/20261102T162849Z-49288.

Read-only audit found a real process-wide fixture conflict. The media close
test creates an unopened ReviewCollection, then closes the real Core service.
Its separate parallel suite can therefore close a host held by the serialized
ReviewCollectionStartupTests suite. Both cases used the same process in the
original VM run, but their exact overlap and historical failure cause are unknown.

The single media-close case now extends the existing serialized Startup suite.
Only the two required nested fixtures become fileprivate and their references
are qualified. Every assertion, awaited task, gate and cleanup is unchanged.
Production code, U114 tests and timing limits are unchanged. Independent review
cloud_smoke_review found no required repair. Commit: 0ce5b0ea.

Before the change, standalone Swift923 passes the NoCollection case and
combined Swift47937 passes both cases. Neither reproduces the original failure.
After the move, swift-test/20261102T163112Z-51142 passes10/10 selected media
and NoCollection tests with zero skips. The two real-close/host cases execute
in order in the same serialized suite. Privacy50815 passes.

The original full gate and VM Unit result are retained as failures. This repair
removes the source-proven conflict; it does not establish that the entire VM
suite is repaired or accept a unit or milestone.
