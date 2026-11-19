# Member deletion in Meguri web

Starting commit `93d9e7fa5`. Implementer session
`s-20261119T070146Z-4b41f2e4`. The HTTP test at
`sync-server-test/20261119T105606Z-71769` failed on the original service
with HTTP 404 where deletion requires HTTP 200.

Filtered exports `cleanroom/20261119T105635Z-77687`,
`cleanroom/20261119T110604Z-23736` and
`cleanroom/20261119T111348Z-60152` implement the administrator-only
action, durable cleanup and the descriptor-relative purge. Independent
reviewer `registry_review` verified the complete source and confirmed
that the purge accepts already-removed descendants while rejecting a
changed account directory binding. Enabled, disabled and provisioning
members can be deleted. Administrator rows and all unrelated storage
remain outside this operation.

The initial browser run `sync-web-verify/20261119T110838Z-35836` exposed
an ambiguous text assertion after the new button added an accessible
label. The assertion now selects the member heading by role.

The focused Docker checks passed. The server suite
`sync-server-test/20261119T111925Z-74023` passed all 206 tests. The web
suite `sync-web-verify/20261119T111925Z-73976` passed the account journey
and its accessibility checks against the actual server over HTTPS.
Staged privacy receipt `private/20261119T112116Z-79455` and similarity
receipt `similarity/20261119T112154Z-84426` passed.

The initial staged privacy scan `private/20261119T112020Z-77121` requested
exact public fingerprints for the two new deletion modules. The
independent review classified all changed sources as public. Reviewed
files have these SHA-256 digests.

| File | SHA-256 |
| --- | --- |
| `server/src/accounts/deletion.rs` | `1d0b85ee5ad9302685654ab0212d53a0d5feae132eba1f219a812e1cfd4cf270` |
| `server/src/accounts/mod.rs` | `346cd0468f349b31790e32cf1c62f93169cc810a8fb75af4646338a41285052b` |
| `server/src/httpapi/mod.rs` | `3053d0d96124426ef439d115d9fcaecde48e3d378fcbaa99b1b921fefc57196d` |
| `server/src/protocol/media.rs` | `e5749fb7305e0964f79982d2dab75338f8d10074ab9a77f1dc81a31dc426fa74` |
| `server/src/storage/deletion.rs` | `22f4dbd29832312191f660ee2f0239bf1e7ce34389dd3e5a06aaa04f8718fe93` |
| `server/src/storage/mod.rs` | `52d2c8c435b40313b3cab34269109c8538ba01d5f9794ae92e15de87e97d5751` |
| `server/src/storage/owned.rs` | `7bdf49fb5c24ab909be7c46b1231539cfefe61491c69dc90ee25163a0abd6a4d` |
| `server/src/storage/root.rs` | `e7f20fb65aeb45ae6e75c14c4b1eb5784608881a1126de47080e36da91bbea64` |
| `server/src/storage/semantic.rs` | `f5a3cc2e74103e01b6c38f407152892f38ef87f69b21b8a908fc7b2dc9de65b2` |
| `server/tests/deletion.rs` | `6f67b407b16d5dc08a61d7feec4ff754fd9ac0cd50f463439f6ead0275d8f266` |
| `server/tests/media.rs` | `e69e8be1cac53f5d20884055ae4cced7a5ea33d460994961f43427641584a752` |
| `server/tests/media_recovery.rs` | `abdd015e0362d864562251ed134d8b768f90b7f89ec4c30f718a84de6155c063` |
| `server/web/scripts/audit-assessment.json` | `8b4d6446cf91a819dc37742d9a0f98dee4b0547917280765724b87d4e4c2e843` |
| `server/web/src/accounts.tsx` | `e885d0be7493201c62715620ba5a0de66762d9b35a8f414ba5943a0cfeb397c9` |
| `server/web/tests/browser/accounts.spec.ts` | `37ca8fc2e940b4b4a42231522dc1a51b24bb7a67c30b97caf8d99ab0b828622f` |
| `specs/servers/meguri-sync-web.md` | `1d7a1e41539e81871ed19bcc02866d99104bf61c8ab35f4927a4874dce91efb2` |
