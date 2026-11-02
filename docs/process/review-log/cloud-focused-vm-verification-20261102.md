# Focused VM verification

Status: tooling checks pass; native execution remains queued.
U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Source start: b0e80d60.

## Whole Mac Unit mode

Implementer s-20261101T235351Z-264d9cd0 produced filtered55484.
Commit7c019b7d adds `xcode-check --mac-unit-only`, using the complete existing
Meguri Unit Debug plan, actual Rust and native helper producers, input identity
receipts and the dedicated VM. Other arguments refuse before work. Normal
matrix behavior is unchanged, and focused receipts explicitly exclude it.
Independent review cloud_smoke_review found no required repair.

Selftest57878 passed24/25 wrappers. Its last case used the real build function
instead of its generated stub. Fixer
s-20261102T142603Z-4626b865 moved only that stub in filtered58383; all
assertions and production code are unchanged. Independent review is clear.
Commit182c2800; selftest/20261102T170127Z-61097 passes25/25, zero skips.

Actual xcode-check/20261102T165453Z-58865 has focusedMacUnit scope and
warn/queued status: zero executed, one skipped. SSH did not answer in240s.
No fixture producer or Unit case ran. This is not a passing Unit receipt.

## Retained crash diagnostic

Fixer4626 produced filtered56207 and repaired it in filtered59912.
The utility admits only an existing Mac UI run and its drive suite. Guest
reports are read and parsed in the VM; public output is a fixed process,
signal and system-library taxonomy. Raw reports, stacks and private paths
are not exported. It is registered as a diagnostic utility, not a gate lane.

Round1 review found two P2 issues: the actual log has no timezone suffix,
and cancellation did not prove guest completion before lease release. Round2
accepts unique-PID attribution with the bounded original UTC run interval
and the original worker/group/foreground guest completion checks. Unknown
completion preserves failure and the existing persistent lease fence.
No timezone is inferred, no unrelated process is killed, and no VM starts.

Commit905a2bb5; privacy61980 passes. Selftest/20261102T170844Z-62285
passes7/7 wrappers, including the generated parser and ownership checks.
Actual diagnostic execution remains unrun because SSH is unavailable.

## VM recovery

After the original focused job ended, root took the idle dedicated VM's
exclusive lease and restarted it headlessly. Readiness again expired.
A single connection through the existing VM console helper reached its
desktop, but did not restore SSH. Root ended that connection and restored
plain headless mode under the lease. No simulator was manually started,
shut down or erased, and no host settings or other project data changed.
Private maintenance evidence stays in ignored, protected build directories.

No full gate was repeated or accepted. Signed iCloud execution, the original
batch gate, U114's recorded failure and required owner evidence remain open.

After Antoine reported further restarts, doctor/20261102T172708Z-65216
confirms the VM answers again and the host session is unlocked. It still
reports the host/VM Xcode version difference and missing Xcode account.
Physical inventory ui-test/20261102T172709Z-65434 finds one available iPhone
on iOS27.2. No device installation, account setup or iCloud result is claimed.
