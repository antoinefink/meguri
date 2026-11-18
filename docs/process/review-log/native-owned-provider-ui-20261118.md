# Owned normal Mac provider UI: focused verification

The complete owned normal Mac setup-controls case passes1/0/0 in
`ui-test/20261118T043428Z-61584` on clean
`7b02e83a99519f8ba9cf9c112a6878e0858a08ec`. Its whole affected tooling file
passes10/0/0 in `selftest/20261118T043422Z-61162`. This focused evidence does
not clear the separate failed Snapshot proof or whole app. Preserve failed gate `gate/20261116T005859Z-94440` and integration base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. No whole-app, sign-in, native review,
Mobile, Unit or full-gate acceptance is claimed.

## Scope

This test-only work began from `65d3ac318264e91ee0bded55ac7efffbc2b85379`.
Seven owned-composition files landed in `d1756203`; throwing-call hoists landed
in `69385a28`. The lane uses the public `local-review-1` recipe, real
ReviewCollection/SyncCoordinator, actual bundle configuration, nil sync
defaults and disposable collection/provider/defaults ownership. It supplies
no fake provider, credentials, Ready state or automatic consent. Production
composition and Core admission remain authoritative.

The exact UI case is
`MacHomeHierarchyTests/testNormalMeguriSetupControlsAndCancelledServiceSwitch`.
The helper case is
`LaunchOptionsTests/ownedProviderFixtureIsIsolatedAndInertByDefault`.
The enabled earlier Snapshot native-control proof stays separate and failed,
including `mac-hosted-test/20261118T014329Z-81784` on `959906ee`. This new lane
does not replace its assertions or infer product absence from missing AX data.

## Actual chronology

Receipt sources were clean unless explicitly marked dirty below. Every failed
receipt remains failed, even when it reached a later boundary. Counts use
passed/failed/skipped.

| Receipt | Source | Actual result and bounded finding |
|---|---|---|
| `mac-hosted-test/20261118T030509Z-7369` | `69385a28` | FAIL0/1/0. Cleanup success assertions failed; root removal, marker absence and unrelated context isolation passed. Cause was not classified yet. |
| `mac-hosted-test/20261118T031725Z-15112` | `d66c0b23` | FAIL0/1/0. Fixed marker: `synchronized=true domainNil=false domainEmpty=true`. Nil-only cleanup rejected a successfully synchronized empty owned domain. |
| `mac-hosted-test/20261118T032221Z-22357` | `4ee1968f5669ec3c582edada527f9bd345ba9968` | PASS1/0/0. Minimal zero-key repair accepts nil or empty only after actual successful synchronization; nonempty still refuses. Original assertions/ownership remain. Temporary print removed. |
| `ui-test/20261118T025804Z-3929` | `69385a28` | FAIL0/1/0 at initial Home lookup. No Settings action credit. |
| `ui-test/20261118T031755Z-17296` | `d66c0b23` | FAIL0/1/0. Fixed metadata found hook error, no Home and one window. No raw message/hierarchy entered this record. |
| `ui-test/20261118T032344Z-25124` | `8afc55de` | FAIL0/1/0. Exact static error classifier returned unknown; no Core/AX cause established. |
| `ui-test/20261118T033243Z-31731` | `0a2e6015` | FAIL0/1/0. Fixed category identified strict fixture-argument refusal. Test URL lacked the directory hint used by the exact registered source. |
| `ui-test/20261118T033757Z-37534` | `89c217f8` | FAIL0/1/0 after directory-hint repair. Real Home, Settings and popup reached; menu-caption reader still failed. Parser/source guards unchanged. |
| `ui-test/20261118T034608Z-42100` | `792a0935` | FAIL0/1/0. Owned menu had exactly two items; both known captions occurred once as titles, zero as labels/values. This justified the title reader only. |
| `ui-test/20261118T035202Z-46220` | `7b132e4f` | FAIL0/1/0. Strict title reader advanced to service-selection failure. Complete two-option/role/uniqueness/action checks remained. |
| `ui-test/20261118T035959Z-50606` | `20000606` | FAIL0/1/0. Intended Meguri item click passed; committed selection was not established. Fixed observations found zero setup fields, change-failed state and no-service footer. Persisted Core selection was not read. |
| `ui-test/20261118T040809Z-53900` | `20000606` | Signed attempt FAIL0/1/0. RunningBoard launch refusal preceded the intended UI journey. The aggregate count grants no functional case credit. |
| `selftest/20261118T043228Z-60054` | `ea41cb2b`, dirty test-only composition | Expected-red FAIL7/3/0 before the exact four-tool repair. All seven old tests pass; the three new scope/parser/profile-delivery tests fail against old implementation. |
| `selftest/20261118T043422Z-61162` | `7b02e83a99519f8ba9cf9c112a6878e0858a08ec` | PASS10/0/0 for the whole affected profile Bats file, including old strict guards and all three new regressions. |
| `ui-test/20261118T043428Z-61584` | `7b02e83a99519f8ba9cf9c112a6878e0858a08ec` | PASS1/0/0 for the entire exact owned normal case, using the actual bound team-signed producer. Every original assertion remains. |

Directory repair changed only the URL producer. Native title repair retained
the same picker menu, exact complete caption set, unique native menuItem
roles, enabled/hittable checks and intended click. Neither adds an option,
fallback, retry, extra wait or consent.

## Producer diagnosis and scoped repair

Public inspection of actual ordinary `035959` product found ad-hoc signing
and `MeguriCloudSchemaConfigured=false`; its declared container was nonempty
and environment valid. The actual bundle bootstrap's false-schema guard maps
to configurationUnavailable. ReviewCollection's service-selection preparation
refuses before handoff. Local Home readiness does not prove that platform
prerequisite. This is a producer boundary, not evidence of a missing custom
server API. Actual persisted selection remains unknown.

Signed `040809` remains a distinct launch failure. Bounded profile comparison
found its current valid automatically selected profile did not cover the VM;
the existing valid pinned app-only profile did. No device ID/name, profile
body, signing subject or private path is included. Retained comparison opaque
SHA256: `f984c162b13d8a70a7d9230f0ded0b67f1aefbeaa87df5d378a5b9e4fa5bd752`.
Coverage booleans establish neither final admission nor a UI pass.

The four-tool repair now consumes the existing validated pinned app-only
profile for only this exact MacDebug/no-device method with one parsed
selector. The invocation-local selector resets ambient values and cannot
leak. Existing profile validators/hash checks, private bindings, app-target
settings and after-build/pre-VM recheck remain. CloudAgent/Snapshot/drive,
excluded jobs/tuples and widget/test signing retain their previous scope.
No profile/schema/Portal mutation, validation bypass, Core change or new
public opt-in was used. It reuses existing actual signing authority.

The passing entire UI case verifies real Home/Settings, the full two-option
native TITLE menu, actual Meguri choice, real username/address/SecureField
controls, and disabled sign-in for empty fields. Native Return cannot confirm
the service switch; Cancel retains Meguri and its setup controls. Graceful
Quit completes through the actual collection close and owned cleanup. The
case enters no credentials and performs no sign-in, network sync, review or
FSRS enablement. It establishes no cold persistence after successful Quit;
the disposable fixture is cleaned. Actual retained Core state was not
independently dumped or read for this UI claim.

## Source and PUBLIC reviews

Complete final-content independent reviews are retained in
`build/cloud-resume-diagnosis/first-delivery-composed-20261117/`:

| Review files | Reviewed scope |
|---|---|
| `owned-provider-vm-ui-composition-review.md`, `owned-provider-vm-ui-options-review.md`, `owned-provider-vm-ui-native-fix1-review.md` | Initial seven-file owned composition/options/helper/native case; source CLEAR after review fixes, no execution claim. |
| `owned-provider-vm-ui-unit-hoists-review.md` | Two throwing-call hoists; original operations/assertions preserved. |
| `owned-cleanup-observe-review.md`, `owned-cleanup-empty-fix-review.md` | Fixed Boolean observation, then observed zero-key repair. Final helper SHA256 `7972f8a6dd3e38a7aabaab02ba7c4df2059fd74474961b471c26a3f21dc853ef`. |
| `owned-hook-error-observe-review.md`, `owned-hook-native-categories-review.md` | Fixed-category diagnostics only; unknown/ambiguous remain unknown, no raw attribute extraction. |
| `owned-directory-url-fix-review.md`, `owned-popup-title-fix-review.md`, `owned-selection-observe-review.md` | Exact URL producer, observed native title reader and failure-only counts; complete-file PUBLIC/CLEAR, not runtime success. |
| `owned-ui-profile-scope-code-review.md`, `owned-ui-profile-scope-policy-review.md` | Complete final-content reviews cover all four tooling files: app.sh/uitest.sh and profile Bats/tools instructions. Both CLEAR/PUBLIC; strict source tuple/parser/hash/private-delivery checks preserved. |

This public record contains no raw UI hierarchy, error payload, collection,
preferences, credential, account, private SDK output or profile body. Receipt
identities/counts come from public MG JSON; bounded classifications come from
approved fixed markers or Root's sanitized actual-producer facts. Source
review never substitutes for execution.

## Separate Core success and next boundary

Real CoreHost/packaged-server `meguri-provider-real-server/20261118T005659Z-63600`
on clean `68d0ffb370cf3b070051d8f30aeef0708bf9c447` passes6/0/0 with canonical
package `005519Z-61832`. Independent actual public-artifact review is CLEAR:
`real-review-six-actual-review.md`, SHA256
`441f2ff727e3288f55a013c30db0b6829fa461402e3e90d26922e05c231722a1`.
It proves its own real Core review/send/receive/cold-retention phases, not
native Reveal, these Settings actions, credential entry, Mobile or an app gate.

Next: complete native sign-in and daily review with the owned custom server,
exact persistence and required integration checks. The passing Settings case
does not supply those missing operations. Keep the earlier failures and the
separate failed Snapshot proof. Original gate/base, existing own-server
register row and final evidence remain unchanged.

Independent actual verification review `owned-ui-actual-verification-review.md`
(SHA256 `7bfdea3a481333833c1a5f37f55896966cf2fdf130b93c441fcbbe48f50ce40e`)
is CLEAR for the exact passing case and tooling receipt. The signing witness
`owned-ui-signed-product-witness-20261118.json` (SHA256
`913f16d7d08a5cc0b2879a624530a449a4b74a426f94eff3736f3bbe7b2701b7`)
checks the post-run host product signature, expected profile and VM coverage.
It is not independent attestation of the guest binary.
