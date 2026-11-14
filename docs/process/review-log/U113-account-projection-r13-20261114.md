# U113 account projection repair: round 13

Start: `78ccf827`; integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Further repair is authorized by `repair-rounds-authorization-20261114.md`.

Core records a temporary iCloud status before Swift returns an error. That
return omitted the policy result, leaving old status and controls visible.
The fix forwards the result and error together. It neither restarts delivery
nor completes the read twice.

Native observations now inspect original Saved bookkeeping before admitted
account completion. Restoration checks accepted account proof, the required
binding-specific Resume, and actual SDK authority. Adapter replay is a separate
fact. All five cases, ownership checks and time limits remain.

Independent cleanroom records:

- Diagnosis: `203525Z-98073`; native author/review: `203519Z-97477`, `204055Z-3427`.
- Production author/review: `204100Z-3804`, `204321Z-5786`.
- Finishing author/review: `204457Z-8604`, `204632Z-10153`.

All reviews report no P0–P2. Root compared the finishing bytes: one explicit
enum qualification and one whole comment removal; functional source is unchanged.
The initial compiler and publication failures remain recorded at
`swift-test/20261114T204350Z-6642` and `private/20261114T204357Z-7646`.

- `swift-test/20261114T204658Z-10546`: three CoreBridge tests pass, including five cases.
- `swift-test/20261114T204747Z-12794`: all 33 native tests pass, including five cancellation/account cases.
- `private/20261114T204705Z-11542`: all four source files pass.
- `similarity/20261114T204733Z-12446`: four files, no flags.

The original StopEngine failure and new account checks now pass together.
This repair does not close U113, P24, the batch gate or the peer-device trial.
