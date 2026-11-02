# CloudKit profile authorization

Status: focused checks pass; actual product check still requires execution.
Component start: 5e427bd4. Original integration base remains1416a520.

Build-app/20261102T181638Z-18401 compiled the personal Mac preview but failed
its profile check. Bounded metadata inspection found exact signed development
claims, while the profile used a service wildcard and an environment list.
This diagnosis supplied no passing provisioning or SDK result.

Implementer264d exported filtered24848 from clean5e427bd4, original16967
exit0. The two host checks now recognize those specific authorization forms.
The app still must claim exactly CloudKit and Development. Container and push
checks remain exact, as do identity, signer, device, expiry and schema checks.
No generic wildcard matching or Swift runtime change was added.

Apple TN3125 describes profile entitlements as permissions, distinct from
executable claims: https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles
Independent review cloud_smoke_review found no required repair.
Selftest/20261102T183416Z-27004 passes10/10 wrappers, zero skips, including
negative grant and signed-claim cases. Privacy26606 passes. No full gate,
CloudKit execution or U113 acceptance is claimed. Development readiness stays
NO until actual grant verification completes; production stays NO.
