# Signed-agent delivery component

Status: focused checks pass; actual signed VM delivery remains unrun.
Integration start: `9ea7ea0b9cc1ddb3222dc5c6bc7b3de684e5bdb9`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261101T235351Z-264d9cd0`.
Fixer: `s-20261025T162405Z-9dc656e4`.
Contract: `conformance/cloudkit-agent#J2.1`, J2.2 and Q13.1.

Resumed prepared tooling through filtered exports `cleanroom/20261102T062709Z-50679`,
`cleanroom/20261102T071901Z-94552` and `cleanroom/20261102T073551Z-19577`.
The child allocates its delivery receipt before freezing the header. It checks
the frozen destination map, actual VM identity, provisioning identity, product
inventory and original test configuration. Copying and publication retain file
and directory descriptors. There is no recursive pathname deletion. Successful
delivery requires inventory readback and actual installation observations.

Independent reviewer: `native_bounds_review`. Round 1 identified three P2
findings: a map could not contain all reserved destinations; provisioning
identity used the wrong VM observation; copy authority ended before pathname
publication. Round 2 found no remaining required findings after their repair.
The generated tests execute the guest payload and exercise ancestor replacement,
entry shapes, bounded reads, hashes, links and publication. They do not prove
the VM's actual toolchain or installation behavior.

`selftest/20261102T065619Z-77162` failed one of 123 wrappers: two nested Python
cases required a registered test module and the expected refusal class. The
repaired `selftest/20261102T073041Z-15299` passed all 27 focused wrappers with no
skips. Its validation bodies are unchanged by the final two usage-text edits.
`private/20261102T073837Z-21271` passed for the combined ten-file candidate.
`selftest/20261102T073841Z-21521` passed the delivery and VM wrappers but failed
the separate import guard's Unicode test fixture; that guard is excluded from
this commit and retains its own failed receipt.
`similarity/20261102T074751Z-28894` passed for all three Python files.
`static-checks/20261102T074715Z-27869` passed all three checks. The first
similarity attempt could not reach the Docker socket from its sandbox; it
executed no scan. The subsequent run used the required OrbStack access.

The parent still lacks command coordination, final receipt-copy delivery and
fixture installation. Account checks and SDK runtime evidence are separate
requirements. This component emits no passing signed scenario cells. No Unit
trailer, full gate or release acceptance is claimed.
