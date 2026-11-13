# Native policy exchange: original failure evidence

Start: `40b8dd5d48e5bda0b768e17cbdeb17464452603e`, clean master.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Specs: `sync/policy#P4`, `#P12.3`, `#P12.6`, `#P15`;
`engine/jobs#J2.1.1`.

The policy exchange now carries its issued command, retained execution and
original SDK witness through both fetches. It returns the original typed error
and sealed outcome after required Runtime and policy completion. Runtime reads
the underlying error for cancellation accounting. A later completion failure
keeps its existing priority and has no earlier qualification. Manual exchanges
still have no issuer. No classification rule, guard or timer changes.

The outer executor retains its task before capture. Output ACK and observer
awaits remain. The adapter installs its worker before the cancellation handler;
no throwing check, awaited lookup or admission failure is added after capture.
Six native cases cover caller-first cancellation, provider-first cancellation,
provider failure, earlier read failure, SDK code 20 and post-discard cleanup
failure. They assert real Runtime capture/results, original error identity,
exact command matching, read completion and required output ACKs.

Three selected exchange fixtures now supply the required canonical equal-Epoch
reply through native callbacks and Core intake. New gate waits use the existing
terminal cleanup pattern. Original assertions and deadlines remain unchanged.

Filtered source: `cleanroom/20261113T202852Z-75676`, then
`cleanroom/20261113T204027Z-83134`.
Review1 `cleanroom/20261113T203742Z-81293`: test cleanup defect and a P1
capture-sequencing question. Review2 `cleanroom/20261113T204532Z-84860`:
no P0/P1/P2 code findings. The retained outer task covers cancellation before
adapter worker creation. The review also required more precise capture wording.

Evidence retained:

- `swift-test/20261113T203756Z-81777`: failed, including an early-ended gate
  wait. Stopped only its owned helper; receipt and log retained.
- Clean baseline `swift-test/20261113T204135Z-83601`: old exchange fails before
  capture, while the explicit equal-Epoch case passes.
- `swift-test/20261113T204546Z-85255`: all six selected methods and thirteen
  parameter rows pass on the final source retained here.
- `swift-test/20261113T204632Z-86401`: provider/cleanup and explicit Epoch
  refusal methods pass. Existing malformed-action setup fails before its
  assertion. Clean baseline `swift-test/20261113T204813Z-88779` has that failure.
- Attempts `cleanroom/20261113T204803Z-88420` and
  `cleanroom/20261113T205231Z-91930` do not land. Native checks
  `swift-test/20261113T205006Z-90407` and `20261113T205451Z-93008` fail.
  Review `cleanroom/20261113T205437Z-92608` identifies a baseline helper that
  rejects the automatic replacement before Epoch intake. These setup attempts
  are not part of the final code. The original negative test body is restored
  exactly; its setup failure stays open.
- `static-checks/20261113T204711Z-87407`: three checks pass.
- `similarity/20261113T204725Z-88048`: four Swift files pass, no exceptions.

The flush needs its own cancellation evidence. Original parked-flush expiry
and successor-ready checks, second-fetch proof execution, internal completion
failure injection and the integration gate remain due. Other native fixture
gaps remain. This is WIP; no Unit, signed acceptance, milestone or release claim.
