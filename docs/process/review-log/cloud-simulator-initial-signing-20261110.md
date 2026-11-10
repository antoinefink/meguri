# Initial simulator development signing

Main starting commit: `7e2f3dbb`. Infrastructure WIP in the pending iCloud batch;
no unit, native signing or signed-sync acceptance.

Native simulator builds retained ad-hoc signatures despite the requested
development identity. J2.1 permits final signing inside the original native
build before its first freeze. Independent review confirmed that interpretation;
no account, certificate, profile, entitlement grant or verifier change is added.

Only the explicit team-signed Debug CloudAgent simulator route invokes the new
helper. It binds current source, actual products, resolved settings and existing
private-key certificate authority. Known app/test/widget code and native preview
libraries are signed inside out. Original signing children are joined through
cancellation and timeout before product authority is released. Identifier and
entitlement semantics, unrelated resources and source remain checked. Existing
final signature/grant verification and first-freeze inventory remain unchanged.

Review repairs added the actual widget/preview members, original-child lifetime
checks and fresh no-follow output binding for native executable replacement.
The fingerprint keeps native code and load commands. It requires the original
`__LINKEDIT.vmsize`. A change fails until actual native evidence supports it.
The signer must have a valid private key, matching DER team and development
certificate, and a valid expiry. Multiple matching keys still fail.

Independent review is clear of P0–P2 findings after those repairs. Combined
tooling check `8165` passed all 31 cases, including signature allocation models,
code/resource mutation refusals, signer cases, cancellation/timeout and refusing
an existing frozen product. These are generated fixtures, not native signing
evidence. Actual simulator build/signing and cross-device CloudKit remain pending.

Only local names changed. Checks and behavior stay the same. Focused run `10472`
passed both simulator tooling cases.

Actual focused simulator build `23812` refused `initial-code-changed` before
its first freeze. Read-only jobs `27494` and `28131` retained the failure and
found only the first signing target had a development signature. Its current
fingerprint equals the retained SDK input, which does not establish the exact
intermediate native pre-sign bytes. No signing limit is relaxed from that
comparison.

An independently reviewed diagnostic retains exact before/after bytes in memory
within the original signing transaction. On a fingerprint mismatch only, it
writes a private exclusive file with fixed Mach-O fields, hashes and byte-offset
counts, then preserves the same refusal. It exports no code bytes or account
values and never affects admission. Focused tooling run `31486` passed both
cases, including the three new generated diagnostic tests. Actual native delta
capture remains pending.

Focused native build `37103` retained the exact transaction diagnostic and
refused the same check. For both slices of the first injection library,
`__LINKEDIT.vmsize` grew from 16384 to 32768 as the signature grew. Signature
start, segment start and virtual address stayed fixed; the diagnostic found
zero other changed bytes before the signature. This proves the specific
allocation change, while the earlier SDK comparison did not. A narrow
pairwise allocation repair is being prepared; the original failed run stays
failed, with no frozen proof.

The pairwise repair preserves the strict fingerprint fast path. Its fallback
requires unchanged slice identity, signature start, segment start and virtual
address, zero other changed bytes, equal positive signature/file-size growth,
and both virtual allocations exactly rounded to 16 KiB. It restores only that
proved allocation field in a comparison copy and still requires the complete
existing fingerprint to match. It permits neither arbitrary allocation changes
nor code, load-command or slice changes. Native signature, grant, source and
first-freeze checks stay unchanged.

Independent review found no P0–P2 issue. Focused tooling run `41267` passed both
cases with five new generated allocation tests, including both measured slices
and the fat layout, plus mutation and malformed-input refusals. The final source
privacy scan passed. Actual native signing remains pending for this repair.

Actual native build `42000` passed allocation comparison and reached the
unchanged final verifier, which refused `product-signature`. Read-only run
`45021` proved a valid development signature and the expected certificate/team,
but the signed app and native entitlement input lacked the team/application
identity fields. Existing CloudKit, container, environment and push grants
matched. This was not a certificate failure.

Independent review approved app-only identity composition as a J2.1 pipeline
refinement before the first freeze. It derives only the two missing identity
assertions from the already verified signer and exact resolved app identity.
Wrong types, wrong values or conflicting aliases fail. Every original service
grant stays in the expected map; typed binary-plist comparison also distinguishes
Boolean from integer values. A private no-follow input is pinned through the
original joined signing call. Other code paths and final verifiers stay unchanged.
Focused combined tooling run `58031` passed all three cases, including seven new
identity/input-lifetime tests. The source privacy scan passed. Actual native
verification of this composition remains pending.

## Role-specific native grants and first-freeze lifetime

Starting commit: `89c84f54`. The existing prepared patch was applied. Antoine
requested direct work; this integration has local review, not a new independent
review.

The prior native audit `67564` showed the ordinary app signature and normal
xcent missing the source's CloudKit/push grants, development App Group and
debugger permission. The app and widget simulated xcent files retained their
own source requests. Missing app and widget groups did not prove a match.

Preparation now binds each role to its exact entitlement source, actual named
resolved settings and normal/simulated native xcent files. It preserves every
normal typed value and admits only the role's declared source requests. The
app's extra debugger grant is a permission change, limited to explicit team
Debug, test hooks, testability and CloudAgent injection. It is not treated as
identity metadata. The widget gets only its own development App Group, with
no app, cloud, push or debugger inheritance. Conflicts and wrong types refuse.

Source, native-input and settings descriptors stay retained through joined
signing and the first manifest publication. File owner/group checks were added
during local review. The existing freeze publisher was extracted unchanged so
the simulator helper can call it inside that lifetime. Mac retains its prior
path. There is one simulator first freeze. Final product, agent and frozen
manifest checks are unchanged.

Focused run `20261110T205610Z-18415` exposed incomplete generated fixtures:
an empty product tree was rejected before freeze, and a certificate mock also
replaced Info.plist bytes. Fixtures now supply actual generated product entries
and mock only certificate extraction. Assertions and refusal behavior remain.
Run `20261110T205658Z-18740` passed all four affected wrappers. Final combined
run `20261110T205816Z-19139` passed all eight selected wrappers, including final
input admission and Mac-profile tests. Privacy `20261110T205817Z-19589` and
similarity `20261110T205816Z-19099` pass.

Native simulator signing and a full batch gate remain pending. No unit trailer,
round-trip pass or release acceptance follows from these generated checks.

Native run `20261110T210002Z-20162` compiled but refused `initial-entitlements`.
Read-only diagnostic `20261110T210221Z-22050`, bound to that exact clean build,
showed one extra known field in both simulated xcent files:
`application-identifier`. The ordinary files were empty; all other simulated
keys were the role's source requests, with no unknown keys or debugger flags.
No grant values, account data, signing operation or SDK call were exported by
that diagnostic. Its temporary MG route was removed afterward.

The preparation now permits that one native identity assertion only after an
exact typed check against the same role's resolved team and bundle identity.
It is removed from the source-request comparison, not copied into either role's
grant map. Existing app-only identity composition remains separate. Wrong team,
wrong role, wrong type and an unknown alias refuse; unknown grants still refuse.
Focused run `20261110T210337Z-22365` passed all four affected wrappers. Privacy
`20261110T210347Z-23038` and similarity `20261110T210342Z-22684` pass. The next
native build is still pending.
