# App-local CloudKit agent delivery

Status: focused Swift admission checks pass; tooling has one failing witness.
Prepared base: a15dd42b34e624371be4efe4e96e151c3340669a.
Integration start: 5ed45bb5c9c235f0f2df880229f752071a4298a1.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer: s-20261101T235351Z-264d9cd0.
Fixer: s-20261025T162405Z-9dc656e4.
Contract: conformance/cloudkit-agent J2, J2.1 and J2.2.

The agent admits the exact device-local run directory before reading run files.
Its OS Application Support root, fixed labels, original header and retained
filesystem authority are checked again at use. Mobile delivery now queries the
actual installed data container. Mac delivery uses a separately selected,
setup-only app-hosted preflight of the exact verified products. That path skips
normal collection and hook startup and observes the OS root and loaded images.
All five original header/product files are copied and read back under the
frozen destination and lease; private account input remains separate.

Prepared authoring export: cleanroom/20261102T114951Z-7313, including the
earlier four-role handoff seed. Independent reviewer: native_bounds_review.
Round 1 found a P1: preflight xcodebuild bypassed the existing VM ownership
and orphan-drain protection. The filtered repair
cleanroom/20261102T121338Z-38674 changes only three files from that candidate.
Round 2 found no remaining required finding.

The repair arms the existing VM test cleanup before launch and confirms a
drain while the lease is retained. Unconfirmed termination refuses handoff.
The original transport, supervisor and process sessions are joined across
cancellation, timeout and lost transport. Generated tests cover these paths
and lease-release ordering. No actual VM process was started by authoring or
review; generated lifecycle tests and native compilation remain unrun.

This component still has no parent command/journal coordinator, approved-account
materialization or complete independent prelaunch counterpart. Its Missing
guards remain intact. It grants no signed scenario, leg, lane, Unit or release
acceptance. The earlier failed full gate remains retained.

Integration removes redundant helper comments and renames two internal names
after the staged privacy check. Call sites and generated source use the same
names; process behavior and scanner rules remain unchanged.

Focused Swift check swift-test/20261102T123819Z-59895 passed 7/7, no skips,
on clean ad4ce8fb. These are generated filesystem/OS-root-admission tests;
actual app-hosted preflight and VM transport remain unrun.
Tooling selftest/20261102T123819Z-59960 passed 13/14 wrappers. Its nested
38-case handoff suite has one failure: cancel-during-launch expects an input
communication event, although cancellation correctly stops and joins the new
child before communication begins. The original child/group and final join
are observed. This test expectation needs focused repair and verification;
the original failed result remains retained.
