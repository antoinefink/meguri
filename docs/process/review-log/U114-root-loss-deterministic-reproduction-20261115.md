# U114 root-loss deterministic reproduction

Starting commit: `5cd11730`. Original gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Production repair and acceptance remain pending.

The complete adapter run `swift-test/20261115T023903Z-38767` reported a stale-runtime error after Active deletion. Read-only diagnosis `cleanroom/20261115T024237Z-40806` traced a possible overlap between callback validation and fresh account-proof acceptance. A single catch-only diagnostic run `swift-test/20261115T025441Z-48501` passed without capturing that overlap. Its temporary source was restored; the earlier failure remains open.

Author `cleanroom/20261115T025825Z-49770` adds a nil-default DEBUG scheduling hook after final callback validation, plus a new test. The test holds a zone-drain callback while the public account-refresh path accepts a real fake-provider result. Before release, required checks prove one provider read, a changed Core guard, cleared root fields and an unchanged cursor. The original tests and J4 observation protocol are unchanged. Independent review `cleanroom/20261115T030241Z-50401` has no required P0–P2 findings.

`swift-test/20261115T030339Z-50907`: one selected method fails at the no-error assertion. It reports the earlier stale-runtime error. All other checks complete. This failing regression remains enabled.

Privacy scan `private/20261115T030450Z-52055` has no findings. Safe implementer report `similarity/20261115T030455Z-52321` compares three files with zero flags. The new regression remains failing until the repair is verified; this is a WIP checkpoint, not a unit pass.
