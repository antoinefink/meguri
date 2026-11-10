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
