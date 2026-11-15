# U114 pending fixture policy opening

Starting commit: `0bbe8523`. Integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The focused Swift run `20261115T011840Z-5508` passed 48 of 53 tests.
Five methods still failed, with nine issues: pending-list equality changed after
lookup events, or the expected send callback never arrived. Read-only diagnosis
`cleanroom/20261115T012335Z-8047` found the common setup gap. Unknown epoch
metadata produces an empty initial SDK list. Accepting complete metadata alone
does not execute the policy's P6/P10.1 gate-opening full update.

The five positive methods now start through the real policy owner, accept the
canonical epoch callback, project the cloud refresh, and execute its ordered
commands. Setup requires exactly one running SDK, one full update, nonempty
pending matching actual sendables, and no fetch or send. Policy and executor
remain retained through the body. Shared Unknown and prepoint cases stay intact.
All original assertions, holds, request counts, errors and timing limits remain.
Both SDK variants are forwarded by the existing native fixture wrapper.

Author `20261115T013149Z-12255` and independent review `013841-14325` prepared
that path. Run `swift-test/20261115T014237Z-16773` then failed before the target
behavior: ordinary fixture opening did not supply the native policy binding.
The strict guard was retained. Fixer `014618-18250` used the existing connected
native opener; independent review `015344-21150` found no P0–P2 findings.

`swift-test/20261115T015818Z-22924`: all five selected methods passed, zero
failures or skips, including both parameter variants. `private/015823-23928`
passed both source files. Broader package and full integration checks remain
required. These focused results accept no unit, P24 item, milestone or release.
