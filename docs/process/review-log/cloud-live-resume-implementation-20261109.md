# S2 native live resume implementation

Source starting commit: `15a0c9c3` (supplied integration input).
Filtered runner starting commit: `51bd6e2d8e04d84aee315b17dd0c3dea70e84d57`.
Original integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer session: `s-20261103T133750Z-f276f58e`. Anki: none.
Contract: `specs/engine/suspend.md` S2.1/S2.2; technical review:
`cloud-live-resume-contract-20261109.md`.

Active resume checks original binding/store, complete present native evidence,
settled scoped publication/storage and current original SDK authority. Its final
owner check is in memory. It retains objects and work; failure denies process
authority without binding or writing a durable isolation reason. Rebind requires
the recorded suspend then connection-release lifecycle, with store/bound absent.

New Rust witnesses reuse the real `cloud_native_absence` Engine/Service fixture.
They cover held original fetch/result, stable current guards versus stale SDK,
missing/unverified/paused resources, native tuple/descriptor/sidecar/read failures,
concurrent owner change, drained rebind, external same-file edit and rollover.
New AppModel witnesses hold original collection/media SDK Service results across
active Core resume and a real local review, then hold the ordinary foreground
account read across Available/settings/visible-media execution and original ACK.
The existing named true/false media replay regression and R11 proof contract
remain unchanged. The SDK fixture gains observation/hold seams only.

One source consistency check only in this runner. No compilation, tests, network,
dependency work or independent review here; root owns those checks and acceptance.
This is implementation WIP under the reviewed S2 clarification, not R2066 policy
approval, a P24 closure, or milestone acceptance. Parallel prepared asset-next
files and algorithms are unchanged.

## Review fixes over reviewed60980seed

Clean integration input: `21c5e02e`; this filtered runner's source baseline:
`62b2f87526808e54770cf5c9bb8007d47c4396c8`. The original implementation start,
batch base and fixer session above remain authoritative.

Root's independent final review requires P1 and P2 fixes. P1 found that the
CloudKit participant propagated observation/refusal errors into local Engine
resume. The participant now retains that cloud refusal and returns local success,
as the existing open-step separation does. Engine's earlier core checks are
unchanged. No bound/store or original resource is cleared to obtain that success.
The negative native, descriptor, sidecar, concurrent-owner and incomplete-release
witnesses now require successful Engine resume and an actual Engine note update,
with original denial, cleanup and no durable-isolation assertions retained.
The unreadable sidecar fixture is restored before its local write.

P2 found that an active publication/adoption transition was treated as new
mismatch evidence. In-memory transitions and recorded operations now exclude
the shortcut without changing authority, operation, refusal or recovery state.
A transition seen at the final in-memory recheck is preserved too. The settled
path still checks complete native/descriptor/publication evidence, the original
SDK and final owner. Mismatch and read errors still deny cloud work.
The active path retains its original objects and grants no new proof.

The added publication witness uses the existing real preview/key-provider/local
publication flow. It covers the recorded operation, held original own request,
and its held actual Service result across resume, then accepts the original
completion once and issues the normal next request. It checks unchanged guard,
stored state, operation, usability/verification and account/Service read counts.
No synthetic receipt advances publication. Existing recovery algorithms remain
unchanged. This is an own-publication witness, not an adoption-request witness.

The added refusal resource witness uses existing publish/adopt helpers, a real
large-note update and Service asset save/ACK. An accepted original asset read
retains ownership across refusal and local work, rejects cancellation as already
accepted, and releases that exact ownership on its refused original terminal
result. It preserves the waiting inbox and explicitly releases its pass and
downloaded transport file. This proves accepted-read cleanup beyond SDK release;
it does not prove removal of an already Runtime-prepared staging file. That
additional staging-file witness remains a coverage gap, without changing the
parallel asset lifecycle implementation or its files.

Root supplied pre-fix evidence: core-lint `80464` PASS4 including all Rust test
compilation, source similarity `80310` PASS, privacy `80327` flagged only the
three new comment blocks in Runtime, the descriptor hook and CloudLiveResumeTests.
Those complete comment blocks are removed here. These receipts describe the
reviewed seed, not this revision. Source WIP and all pending acceptance conditions
above remain. Root must compile/run the changed witnesses, repeat required
checks and obtain independent final review; no test pass is claimed here.

All seven seed files are retained. CloudServiceSDK, the parent Rust fixture,
AppModel's held Service/local-review/Available behavior and the true/false media
replay/R11 test remain unchanged; CloudLiveResumeTests changes only by comment
removal. Timing limits, SDK counts, policy/schema and public API are unchanged.

## Current integration checks

Core `20261109T180114Z-87740` compiles and runs eight live-resume checks.
Seven pass, including local writes after cloud refusal, preserved publication work and external invalidation.
The accepted-asset cleanup case fails during its initial adoption helper, before resume is tested.
Source diagnosis identifies a Service page limit of 50 against adoption's maximum 32.
The exact rejected page size was not captured. A scoped fixture correction remains pending.
All validators, cleanup assertions and deadlines remain enabled.

The combined lint run is blocked by three test-compilation errors in the separate asset lifecycle draft.
Those errors do not occur in this Host test target. Native verification and the original batch gate remain pending.
This WIP source commit grants no unit or release acceptance.

## Scoped cleanup fixture correction

Filtered fixer export `20261109T181921Z-90265`, based on `509662ed`, changes only
this test's Service constructor to a page size of 32. Shared helpers, production
validation, assertions and deadlines are unchanged. Focused core check
`20261109T182549Z-91537` passes the previously blocked cleanup witness. Together
with the seven unchanged passing cases in `87740`, all eight Rust resume cases
have passing focused evidence. Native checks and the original batch gate remain
pending; this is not unit acceptance.
