Resume binding continuation, 2026-11-01.

U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Integration base: d1d2611d496f0903cb3f735ea08230ba4946004e.
Fixer session: s-20261025T162405Z-9dc656e4.

The pending implementation resumes an isolated binding from its retained,
single-use preview. Confirmation preserves tracking and pending edits, replaces
stale runtime authority, settles durable intents and reconciles before starting
without a saved cursor. Native settings enable only after confirmation; a fresh
account read follows. Discovery does not require an existing Resume offer.

Independent review found a nested reader acquisition in preview construction.
Reuse the held snapshot; release the confirmation snapshot before reconciliation.
The earlier cancelled core-test/20261101T160134Z-84462 remains an error.
A synthetic publishing manifest retained counts valid only when complete;
its fixture was corrected without relaxing the decoder.

Local recovery originally rejected a stopping SDK even after its authority had
been invalidated. Keep the current stopping-SDK rejection; an obsolete SDK must
not block local replay. Stale authority and network admission
remain rejected. core-test/20261101T165946Z-30266 passed the focused negative.

core-test/20261101T165521Z-29587 passed 29 selected tests. This includes actual
publication, local edits/reviews, Resume and a 300-pair crash/reopen workflow.
It uses 50-record pages and kills the child after the first covered page,
then reopens and checks convergence. This is not Apple SDK or live device proof.
Other tests unselected: 14.

swift-test/20261101T171014Z-33894 passed 12 bridge and mapping checks.
swift-test/20261101T173643Z-38592 passed 8 of 10 focused native checks.
Both successful Resume cases failed before SDK startup. Diagnostic build
swift-test/20261101T174827Z-41778 failed Swift macro type checking; message
construction was split outside the macro, retaining all conditions.
swift-test/20261101T175448Z-44212 then reproduced both runtime failures:
Core returned authorityChanged before settings enabled or an account read began.

Independent code review traced disabled settings to adapter_pause(true).
Resume must settle accepted media intents before native settings enable, so
the pause predicate made confirmation and enablement each wait for the other.
Independent spec review confirmed V2.2, V2.4 step 8 and K9.3 require local replay.
P6/P11 still prohibit new remote work and new inbox admission while paused.
The shared guard also serves new metadata admission, which needs its own fence.
No normative change or early settings enablement is authorized.

Independent Swift review found no P0-P2 issues in opaque preview ownership,
confirmation, account refresh, failure state or presentation. This is not
rendered evidence. Antoine is resizing the VM; no VM jobs run meanwhile.

Filtered paused-state repair: cleanroom/20261101T180519Z-55627.
It separates existing local replay from new intent admission. READY selection,
new apply entry and final intent persistence require an open device gate.
The final check holds the same state lock as persistence. All current-authority,
tracking-stop and current stopping-SDK checks remain. Strict network checks stay.
Independent R3 source review found no P0-P1 or other P2 findings.

core-format/20261101T181205Z-60916 passed.
core-test/20261101T181218Z-61247 passed 47 selected tests, with 88 unselected.
This includes the three disabled-state cases, corrected other-account refusal,
both actual 300-pair crash/reopen cases and the full publication target.
The attempted module-name filter selected no sync library test; no library
coverage is inferred from that run.
core-test/20261101T181410Z-64534 separately passed both recovery-authority
checks, with 86 unselected. Stale identity/file/token and current stopping SDK
are rejected; local replay never authorizes network work.

swift-test/20261101T181341Z-63031 passed all 10 selected native checks.
Both formerly failing confirmation cases now reach a fresh account read and
cursorless startup. Discovery, cancellation, one-use preview and failure-history
checks pass. This host package run does not provide VM rendering or device proof.

similarity/20261101T181422Z-65011 passed on the complete pending source.
private/20261101T181423Z-65247 flagged only two test-local variable occurrences.
A filtered rename preserves the assertions and all production code.
Final identifier-only export: cleanroom/20261101T181611Z-66891.
Source privacy: private/20261101T181712Z-67696 PASS on all 31 files.
Review-log prose was rephrased after its separate privacy scan flagged three lines.
core-test/20261101T181713Z-67903 PASS3/3, 28 unselected: final test source.
static-checks/20261101T181743Z-68560 PASS.
The test-local rename changes no production input or assertion; the earlier
47/47 and native 10/10 results cover the unchanged behavior.

U113/U114 and the original batch gate remain open. No Unit trailer, P24 closure,
milestone, signed CloudKit, rendered UI or release acceptance is claimed.
