# Sync Now verification for an existing collection

This is a checkpoint for U113/U114. It does not accept either unit or a
milestone. Their remaining requirements and final evidence stay open.

- Slice start: `e9a5ae2207750e1262b88d2fd35d1f1f7fe441a9`.
- Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Implementation session: `s-20261110T214025Z-20fa15fd`.
- Contracts: policy P6, P12.1–P12.7; recovery V26.1 and V3.4.
- Approved clarification: `de8b0614`, policy P12.2. Other review holds stay.

## Result

An existing binding with no qualified epoch date reads its own epoch before
Sync Now captures a selection. Both the direct adapter and native policy use
the original request. Writes stay closed until accepted metadata commits and
the date comparison passes. This creates no extra fetch, root confirmation,
false drain, quiet point or completed exchange.

Early results preserve pending work. Waiting without an error has an explicit
None class through storage, FFI and UI. An accepted refusal survives later
cancellation; a discarded reply grants no authority. A session starting after
an accepted request follows P12.2 and P7. New requests during review stay
unavailable.

The native handoff runs off the adapter actor. Cancellation closes the original
read and joins its worker and callback. Fetches and record reads remain ordered
through acceptance and account proof. Stale replies retain diagnostics without
applying data or changing authority.

## Independent review

Prepared work was resumed with seeded filtered runners on master. The final
export is `cleanroom/20261113T070627Z-61790`; the core changes were written in
filtered implementer/fixer sessions. No reference or private source was used.

Earlier prompts incorrectly named the image-only critic role for source
feedback. Those observations remain recorded in their run artifacts. Required
source coverage comes from the fresh implementer-role reviews below. The
screenshot critic was not used to claim source or visual acceptance.

| Run | Coverage and result |
|---|---|
| `20261112T224926Z-62364` | Initial feedback: FFI type error, missing None result and changed-command handling; repaired |
| `20261112T225840Z-64974` | Session continuation, fetch/read ordering, lock boundary and refusal precedence; repaired |
| `20261113T061714Z-5359` | Remaining contract and test-order qualifications; retained |
| `20261113T063228Z-11546` | Off-actor handoff and revised test order inspected |
| `20261113T064051Z-20129` | Fresh code-role review of all 28 source/test/resource paths; cancellation, discarded native reply and fixture inputs required repair |
| `20261113T065540Z-42015` | Code-role closure of those repairs; no new P0/P1 |
| `20261113T071421Z-68511` | Bounded check of post-review diagnostic and asset-fixture repairs; no P0/P1 or coverage gap in that delta |

The contract writer and a separate spec reviewer settled P12.2 without a new
product decision. Their record is
`cloud-sync-now-session-capture-clarification-20261113.md`.

## Executed checks

Failed runs are retained; none was relabelled as passing.

| Run | Observed result |
|---|---|
| `swift-test/20261112T222818Z-59769` | Initial compiled RED: valid baseline, zero required reads; four assertions failed |
| `swift-test/20261112T224948Z-62821` | FFI compile errors; no native execution |
| `swift-test/20261112T225853Z-65364` | PASS 17/17 after the first repairs |
| `core-test/20261112T225907Z-66333` | 29/30; one old policy expectation failed |
| `core-test/20261112T230121Z-67206` | The same policy test failed on unchanged master |
| `core-test/20261113T061754Z-6835` | PASS 30/30, including exact read-only mismatch actions and old/new waiting records |
| `swift-test/20261113T061747Z-5897` | Held validation stalled actor cancellation; only the verified test helper group received TERM; original MG parent recorded FAIL with incomplete XML |
| `swift-test/20261113T063238Z-11886` | New test macro compile error and deprecated API warnings; no execution |
| `swift-test/20261113T063658Z-16456` | Four issues across three tests; cancellation class and missing fixture responses then repaired |
| `swift-test/20261113T065551Z-42486` | PASS four focused tests, with all parameter cases |
| `swift-test/20261113T065821Z-46630` | 29/32; two asset fixtures and one stale-read diagnostic failed |
| `swift-test/20261113T070257Z-59947` | Clean baseline: both asset tests failed; all stale-read variants passed |
| `swift-test/20261113T071202Z-64260` | PASS all three repaired tests, including five lifecycle and four revocation cases |
| `swift-test/20261113T071433Z-68960` | PASS 32/32: seven registry and 25 CoreBridge tests |
| `core-test/20261113T071629Z-70901` | PASS 369/369 selected tests: policy, adoption, SDK ownership, absence, epoch barriers, media and FFI connection tests |

The asset fixtures feed a Complete epoch record through Core before asset work.
They run setup commands and await their acknowledgements. The tests retain
their checks and LegacyUnknown coverage.

Additional checks: Rust lint `20261113T072306Z-78543` passed (3/3), static
checks `20261113T072311Z-78825` passed (3/3), and similarity
`20261113T072344Z-79549` passed (30/30, zero exemptions). The first privacy
run `20261113T072336Z-79209` flagged comments and prose. Comment-only runner
`20261113T072433Z-79989` rewrote three comments, with all other seed bytes
preserved. A later prose-only finding was removed. The guard is intact.

Staged privacy scan `20261113T072625Z-82418` passed.

## Still open

- Full batch gate with the original base above; all unit and milestone gates.
- Current signed Mac/iPhone round trip, cold-launch retention, rendered UI
  inspection and audibility. Earlier signed results used older code.
- The earlier delayed warm transfer has not been proved fixed by this work.
- K10 status evidence remains incomplete: `attention_known` stays false.
