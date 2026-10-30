# Native iCloud integration checkpoint

Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
U113 start: `1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a`.
U114 continuation base: `babb20ab`. Fixer session:
`s-20261025T162405Z-9dc656e4`. Status: WIP, no Unit closure.

Prepared native execution, media and settings patches were retained and
integrated from filtered source runs. Their source reviews covered plan
conformance, correctness and the clean-room boundary. UI review also covered
native controls. Source review does not establish runtime or visual acceptance.
Transient handoffs remain under `build/unit-intake/U114-native-final-api/handoffs/`
with a SHA-256 manifest; this log records integration evidence.

## Repairs and review

- Original SDK work owns the public asynchronous CloudKit call and its drain.
  Task creation alone is not operation completion or acceptance.
- Local metadata recovery retains binding and lifecycle checks without applying
  the network media switch to local recovery.
- Resume holds callbacks until Core validates the current account response.
  The policy update uses that response without another provider call.
- A zero-progress apply preserves waiting media and evidence, then waits for
  an external trigger. Positive progress keeps its continuation.
- Settings retain unavailable platform configuration across collection
  replacement. Status rows share one native group. Search identifiers attach
  to native controls without changing their appearance.
- Independent source reviews found no remaining P0/P1 in these repairs.
  The missing canonical unknown-account string was registered in `93028713`.

## Verification

All entries below are MG run IDs. Earlier failures remain retained.

- `core-test/20261029T231617Z-58539`: six no-collection authority checks pass.
- `swift-test/20261030T003414Z-3565`: the native partial-error callback check
  passes with its exact delay assertion.
- `core-test/20261030T004846Z-14620`: all four media regressions pass, including
  the two cases that previously stalled and the existing-file variant.
- `core-test/20261030T005821Z-28582`: all 171 FFI/Host checks pass. The explicit
  apply fixtures use real guarded inbox writes; the normal FFI path still
  applies fetched records. Timer cancellation runs through native admission.
- `core-lint/20261030T005944Z-29471`: pass, including all-target compilation.
  Operation facts use one FFI record with unchanged values and validation.
- `swift-test/20261030T005312Z-20105`: the original startup check passes with
  Connected established before Host open and exactly one policy-issued Start.
  Production LocalOnly behavior and pure adapter defaults are preserved.
- `build-app/20261030T005632Z-24992`: standalone Mac Debug build passes.
- `swift-test/20261030T010112Z-31426`: 18 pass, seven fail. Six checks assume
  exchange IDs start at one; the actual ID is five. The suspended callback
  check adds a Deck while reading the account. New Deck capture is unsupported:
  tracking stops, invalidating the original read. U113 remains open.
- `core-test/20261030T010022Z-30691`: 23 pass, three fail. The server comparison
  omits MediaName from its expected set. Two file-replacement reopen cases
  return Engine(Other); the exact cause still needs a typed diagnosis.
- `similarity/20261030T005703Z-26740`: pass for changes since `babb20ab`.
  `similarity/20261030T010217Z-33424`: pass for the final five changed files.
- `drive/20261030T010345Z-34191`: both minimum and default settings captures
  were inspected by the integrator and an independent reviewer. No further
  visible layout findings. Hierarchy still fails the Search child button ID;
  the search field and Form IDs pass. No guard exception is permitted.

Final source follow-ups `005449Z-22900`, `005623Z-24589` and `005809Z-28152`
were independently reviewed: no remaining source P0–P2 findings. The runtime
failures and UI identifier above remain open. No check is treated as passing
because source review passed.

## Open acceptance work

- Original-base full gate, focused checks, privacy and similarity.
- Initial collection adoption and native collection assets.
- Deletion handling and conflict navigation.
