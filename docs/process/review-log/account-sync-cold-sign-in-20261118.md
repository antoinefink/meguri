# Account & Sync cold sign-in repair

Starting commit: `ac08bf8a8ff7e329f76234f6c5a232ceb5762561`.
Role: implementer, restricted source runner. Scope: own-provider native account
setup, `sync/meguri-provider` MP2, `sync/status-strings` T18.3–T18.4,
native-interface-contract N12.4 and `settings.accountSync`.

The owner reports installed signed preview `def06455b`, an empty local startup
and durable Meguri selection without a configured service. Typed logs from the
past click are absent. Source inspection confirms the predicted path: `start()`
returns `.noFile` without a Host; platform preparation reads the selection;
the old form calls coordinator sign-in directly; `meguriConnection()` refuses
without an attached provider before configuration, credentials or HTTP, and its
unavailable error maps to the generic sign-in message. This is source evidence,
not a claim that the historical click or an executed regression was observed.

The form now calls ReviewCollection's explicit sign-in entry. Its retained task
reserves preparation synchronously, invokes the existing NoCollection helper
only when attachment is absent, joins the actual offline load, then checks
coordinator identity, generation, attachment revision, address revision and
lifecycle before calling ordinary coordinator authentication. Close and
background cancel and join preparation. The coordinator still owns its normal
operation, drains, configuration, credential store and authentication task.
Coordinator admission captures address/generation before its task is scheduled.
The helper propagates typed attachment failures instead of losing them to its
final missing-service check. No logging or scanner changes were made.

The form's main task is entering the service address and credentials; its primary
action is Sign In. It uses one localized heading, grouped Form, native fields
and system button styling. Mac rounded-border fields expose input boundaries.
Only redundant ordinary sign-in status copy and repeated provider labels are
omitted here; shared popover labels, special states, download, stop and sign-out
confirmations retain their behavior. Fields and service changes are unavailable
while processing; errors have text, an SF Symbol and an identifier. Password
clearing remains on attempt, address edit and disappearance.

Regression additions use the existing real ServiceAppHarness and a synthetic
credential store. A warm ordinary selection is closed without configuring a
service; a fresh owner then launches NoCollection from that durable selection,
with no backend. The test first exercises the old generic local refusal, then
the new entry twice synchronously. It requires one real Host preparation,
configuration and typed invalid-username login refusal, an actionable localized
message, no collection file, no saved credentials and no transfer calls.
Held backend attachment cases cover background cancellation, address revision
changes, local attachment failure and typed busy failure. Existing presentation
tests gain local-store/attachment/busy versus authentication/network/redirect
classification coverage. No other ordinary caller repeats this pattern.

No builds, tests, network requests or UI driving ran in this sandbox. Source
consistency inspection and `git diff --check` found no whitespace errors.
The installed app has not been replaced. Independent source/privacy review,
exact-hash admission, focused execution, native rendering/accessibility and
provenance/privacy/similarity checks remain for integration. The existing failed
integration register and original batch base remain unchanged; no acceptance
or gate pass is claimed. Generate the inventory's derived `screens.md` through
`tools/mg design-docs --write` outside this runner before verification.

Focused integration commands (activate `.local/activate` first when present):

```sh
NSUnbufferedIO=YES tools/mg swift-test --package MeguriKit -- --filter 'ReviewCollectionStartupTests/(coldMeguriNoCollectionSignInAttachesBeforeAuthenticating|coldMeguriSignInFencesSuspendedPreparation|meguriUnchangedAddressAuthenticationRetryKeepsExactConnection|meguriExplicitAddressEditSurvivesLocalReloadAndStillConfigures|meguriExplicitAddressEditRetainsCredentialDeletionFailure)'
NSUnbufferedIO=YES tools/mg swift-test --package MeguriKit -- --filter CloudSyncPresentationTests
```

Also verify the native form in the permitted off-screen/VM lane before design
acceptance. Close during preparation, rapid background/foreground recovery,
existing ready-collection attachment, stored-address retry, invalid-address
copy, password clearing and special/paused states remain integration edge cases.
Do not drive the signed host UI for this repair.

## Narrow independent-review fixes

Fix round starting commit: `ddc893b41e60dc9a1a4b6a13120983ceac4bddf6`, with the
seeded candidate already present. The integrator reports 47 focused passes and
a signed Release build for that seed; those results do not verify this round.

The preparation catch now preserves `MeguriSignInLocalFailure` before mapping
Bridge busy and credential-store failures. Explicit cold Resume shares only the
existing local sign-in preparation lane, including its MainActor ownership
predicate, cancellation, generation, attachment and address fences. It joins
the offline attachment/load and invokes the existing settings write on that
exact coordinator. Attached pause/resume and Automatic keep their existing
settings path. Settings Resume is guarded while processing; the current Meguri
popover offers no pause Resume action to reroute. No native styling changed.

Added `ReviewCollectionStartupTests/coldMeguriPausedResumePersistsAfterAttachmentAndReload`.
The cold Resume test verifies that the saved pause setting stays off after reopen.

No runtime work was performed in the container. Source consistency review and
`git diff --check` only; execution, independent review and integration checks
remain with the integrator. No new verification or acceptance is claimed.

## Integration evidence

Integration base `6de3f21e4286b9a5be3b8c51c2fd6e2e108c9288`. Implementer session `s-20261118T153159Z-9e5df033`.
Filtered author export `20261118T153200Z-25489` and final reviewed export `20261118T154621Z-52935`, `20261118T155716Z-75220`, and `20261118T160053Z-84183`.
The independent reviewer classified the complete files and ledger row as public.
Ledger SHA-256 through LF `927c96a79bc83c30278f2f5c87f6096fe1d796ce75c2803a12cf719f291e216c`.

The generated screen document differs from its previously reviewed whole-file bytes
only in the Account & Sync entry regenerated from the reviewed inventory by `design-docs/20261118T155744Z-76224`. The exact complete rendered digest is below.

The first Swift compile refused the local ownership predicate without an actor
annotation. The corrected predicate runs on MainActor. The baseline call reproduced
the generic error in the regression. The repaired call reached the actual typed
username refusal without creating a collection or starting a transfer.

- `swift-test/20261118T155748Z-76875` passed 48 focused tests.
- `build-app/20261118T155855Z-76219` passed the team-signed Release build.
- The full integration gate remains pending.

| Reviewed path | SHA-256 |
| --- | --- |
| app/Packages/MeguriKit/Sources/AppModel/Collection/ReviewCollection.swift | 65409c80fcd1cf6b92da941bde842cccd58b396c2cdbc27a6f029f33310f89ad |
| app/Packages/MeguriKit/Sources/AppModel/Collection/SyncCoordinator.swift | 456aa2d9303b4e4a689f40f4ecfdf3692e5ee75612c8319f6652c3f8a5936b68 |
| app/Packages/MeguriKit/Sources/AppModel/Sync/Resources/Sync.xcstrings | b53985ac1a2fc0896b0fc126f348614f962491d763da1bdedab3e309ab0e46f0 |
| app/Packages/MeguriKit/Sources/Features/Settings/Sync/MeguriSyncSetupViews.swift | 10614372d7279294c408d3638b01046d06c999d06d51f0daf413b17e2dee17ff |
| app/Packages/MeguriKit/Sources/Features/Settings/Sync/SyncStatusContent.swift | 9238c2e349fd1c3cb4fcc4d6572209cd4809ccca1440565bf9d26d54f7bcf701 |
| app/Packages/MeguriKit/Tests/AppModelTests/CloudSyncPresentationTests.swift | 9656d70a62e751fe2e4a121a762ccdd6b23313ceb2822e6f696ca3a55f4f2561 |
| app/Packages/MeguriKit/Tests/AppModelTests/ReviewCollectionStartupTests.swift | ff5bfb4b6ba416f753c155e401091d946e811369878242774bd86108b8b08719 |
| docs/design/screen-inventory.yaml | cad9cab5b364992250a8060399849a90efa81d20109e75e145ff914e24cf0361 |
| docs/process/review-log/account-sync-cold-sign-in-20261118.md | 8fe90f0737ad4614e6b601ff9b1344f98acf64357ffa8ade927a5b68cf21f4a8 |
| specs/sync/status-strings.md | 24dd422510bcb309b0cd30d0ff4f44e286f441da5a5284b1ef9acde3432b5b4e |
| docs/design/screens.md | 8075ff26d0014382e34c434f36d8c3dc9974b606fd1d4008df0a953bf39b3ac7 |

## Installed preview

The signed Release app from `build-app/20261118T155855Z-76219` was installed
as the existing development app. Its app and widget signatures and
executables verified against that receipt. The previous app was archived
locally. The app remains closed for the next sign-in.

Privacy, static and similarity evidence passed in
`static-checks/20261118T160505Z-91063` and
`similarity/20261118T160141Z-86273` with the staged privacy receipt.

The Mac VM UI job `ui-test/20261118T160505Z-90578` stopped in approved
profile validation with guarded-input. No UI case ran in that attempt.
The local regression and signed build evidence remain scoped.
