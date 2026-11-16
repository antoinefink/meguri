# Local Docker package and recovery

The local Docker unit is implemented and its focused verification passed.
Final unit source is 8ada1bf5542f8187dd38d40316cf92fcf45769b6.
The actual tested image was built from clean source
8a2f4dac99c8dcb8d612551ef2682d05327ffa75. A later correction changes only
README/help text and public provenance metadata. All 148 frozen package-input
files were independently compared with final source and have identical bytes.
This comparison does not claim a final-source image build.

Original unit start and final coherent batch base:
480df4526514e294f5a3a16dd4ab83a1fbcc7f47.
Workspace: /private/tmp/meguri-sync-local-docker-20261116.
The final coherent gate and final supported-client acceptance remain pending.

## Delivered behavior and license boundary

The pinned Dockerfile produces a release executable with compiled web assets
and a separate test-client image. The scratch runtime uses UID 10001, private
persistent directories, no extra capabilities and the existing ownership rules.
Backend HTTP preserves the configured HTTPS origin, trusted-peer checks and
Secure cookies through the private test proxy. No production TLS service was added.

Packaging retains source/lock/features identities, exact runtime inventory,
Rust and frontend notices, compiler copyrights and MPL-covered source.
First-party server code remains MIT. No Cargo/npm dependency graph changed,
and no app/core behavior or license changed. Dependency notices retain their
own obligations; this unit does not claim exclusive ownership of dependencies.
Backup/restore, deployment automation, publishing and standalone distribution
remain deferred.

## Review and failure evidence

Every production author, fixer and source reviewer used the filtered MG
clean-room runner. Fourteen original role-session append commits remain intact;
the prior 1014-record ledger prefix is unchanged. Independent complete-record
review is retained at /private/tmp/meguri-sync-local-docker-ledger-review-20261116.md.
The resulting 1028-record ledger has SHA-256
5baabed59b8dad56eb20c5ba6df9a15d4b111639c603e13ee2fd68313e97233c.

The unit handoff retains every original session, guarded export and review:
build/local-docker-unit/handoff.json, SHA-256
22ca8f1605dcc6286a337051abdcb0cec8596298d9eceba215aeb316c53203ff.
Its final source export is committed-source.patch, SHA-256
4634c5029e0d7b2d02de3cd6a28ce56697eaa85ceed83a7a58d517e5d51f40b1.

The first review found missing browser execution, unsafe failed-init metadata,
credential-bearing context inputs and mutable static build inputs. Fixes and
independent closure preserved actual browser tests and concrete pinned inputs.
Two helper cases failed before the repair; all four cases then passed.
The browser fixture permission finding was corrected without privileged repair.

Three actual packaging failures remain recorded: static renameat2 linking,
compiler notice layout, and Docker-export metadata classification. Repairs use
the existing Linux syscall without fallback, the installed compiler's complete
copyright files, and exact typed Docker metadata entries. Independent reviews
closed every blocking finding. Actual export characterization accepted the real
artifact and rejected four invalid variants. Four selected publication/recovery
tests passed; this is not a full-suite or every-generation-race claim.

## Actual packaged-image verification

Receipts below are relative to the workspace's build/mg directory:

- sync-server-package/20261116T211724Z-36181/result.json: package passed.
- sync-server-package-test/20261116T211724Z-35923/result.json: 4/4 passed,
  zero failed/skipped. Actual browser setup/login/admin/signout and native
  collection/review/media checks ran before and after same-volume restart.
- sync-server-deps/20261116T211725Z-36438/result.json and
  sync-web-deps/20261116T211732Z-37079/result.json: child audits passed.
- sync-server-test/20261116T204127Z-47810/result.json: four selected tests passed.
- selftest/20261116T202628Z-32224/result.json: all four helper cases passed.
- selftest/20261116T202634Z-32556/result.json: registry check passed.
- selftest/20261116T211338Z-89916/result.json: export characterization passed.

Runtime image:
sha256:80a04cbe5c93317b0a99d9e7ec85b0bbac8d167a85520e5025828706739bcd07.
Test-client image:
sha256:3704abb3e924cecd796c4c8c3b2ca3a5ecdf7754fb163f6ba8b6399e98626b32.
Build target is aarch64-unknown-linux-musl, release profile, features [].
These are synthetic package checks, not Desktop, Mobile or retained-client credit.

## Local command rehearsal and correction

build/local-docker-unit/local-rehearsal-evidence.json retains twelve real CLI
calls against clean tested source: eleven passed and one documented username
recovery attempt was refused. Existing recovery selects an opaque administrator
ID. Using that ID succeeded with the same protected input file, proving the
selector caused the refusal. README and MG help now state the existing contract.
There is no username fallback, lookup framework or stdin guard change.

Four fixture-only calls passed eight browser/native cases across initialization,
restart and recovery. Temporary parent-reviewed MG orchestration was truthfully
recorded as dirty; exact original tiers and source were restored before every
real CLI call. The temporary family was removed. This operational rehearsal used
the same fixed synthetic password. The separate existing Rust storage proof
checks replacement-password success, old-password refusal and preserved bytes.

Correct-ID recovery receipt: sync-server-local/20261116T220046Z-78762/result.json.
Final fixture check: sync-local-rehearsal-fixture/20261116T220210Z-82826/result.json.
Final stop: sync-server-local/20261116T220409Z-85293/result.json.
Both owned host secret files were removed after inode/mode checks. The owned
synthetic volume remains stopped; no service is published, no unrelated Docker
resource was removed, and no source/Git/CPU hold remains.

## Parent checks and remaining acceptance

After complete independent public-content review, only exact whole-file privacy
hash entries were admitted or refreshed under the standing delegation. Scanner
matching, thresholds and private-input access did not change. Final focused runs:

- private/20261116T221553Z-95089/result.json: four files passed.
- static-checks/20261116T221553Z-95389/result.json: three checks passed.
- similarity/20261116T221556Z-96428/result.json: nine supported files passed;
  eleven out-of-scope paths received no similarity credit.

Chrome's browser-access host remains unavailable. Two actual package screenshots
and the earlier nine-screen browser journey are linked in the parent preview
index, with their distinct tested-source identities retained. The actual package
screen has a minor singular account-count copy issue for the final UI review.

Final exact-image Desktop, retained-Meguri and actual AnkiMobile evidence remains
required. No full coherent-gate, final device, production or app acceptance is
claimed here. Original commits remain unsquashed and nothing was pushed.
