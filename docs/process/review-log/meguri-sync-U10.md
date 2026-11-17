# Local Docker package and recovery

The initial local Docker unit and its focused verification completed on 2026-11-16.
Its source was 8ada1bf5542f8187dd38d40316cf92fcf45769b6.
The 2026-11-17 master integration and license correction are recorded below.
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

## Master integration and current MIT package, 2026-11-17

Normal merge 44b8c55d445530915fe80444eb6ae8f0dfa36b7c preserved all 26
original U10 commits. Nineteen nonmetadata paths matched the completed U10
source exactly; the two metadata resolutions preserved the complete main
ledger prefix and all 14 original U10 appends. Independent complete-content
review and parent byte/AST checks passed. No U11 or phone-helper code landed.
The focused main integration checks passed: privacy, provenance, six
package/scanner/registry checks and supported similarity.

Fresh master initially lacked its checkout-scoped web dependency cache.
Package test 20261117T150059Z-92370 failed before any smoke case. Supported
locked install, dependency audit and browser installation then passed. No
versions or lockfiles changed. The next package test, 20261117T150719Z-19691,
failed at the obsolete requirement for an MPL notice. Both failures remain.

The first-party workspace and model now use MIT. Compared with the initial
148 frozen package inputs, only core/Cargo.toml had changed before this repair.
Filtered fixer s-20261116T204829Z-38c8479a corrected the package assertion
and current NOTICE/README wording. It retained complete nonempty approved
license inventories, license text and usage, unmodified model/workspace
source archives, and all input/notice/archive hashes and private-context guards.
Third-party license policy and dependency versions did not change.

Independent reviewer s-20261116T204829Z-eadb1b04 read all four complete
files. MG cleanroom/20261117T152310Z-46891 reported CLEAR/PUBLIC; its report
has SHA-256 ac1f05abb939e9fe43fdb54526e3ae2aecb23515311ab84d23de6577bc1d0981.
Main commits 92b23410e and c145b92861fd67799cbb852cc7b74ed2a78d8136
preserve the unrelated remote, app and Core history. The former updates only
three existing reviewed public hashes; scanner behavior remains unchanged.

Focused main checks all passed: private/20261117T153830Z-63620,
static-checks/20261117T153831Z-63987, six selected tests in
selftest/20261117T153832Z-64478, and three supported files in
similarity/20261117T153921Z-73065. Two unsupported files received no
similarity credit. All receipt paths below are under the main checkout.

Actual clean-master sync-server-package-test/20261117T154144Z-246/result.json
passed four cases, zero failed/skipped, on c145b92861fd67799cbb852cc7b74ed2a78d8136.
The real image build and browser/native collection, review and media checks
ran across same-volume restart. This is current master package evidence,
not Desktop31, Retained8, Mobile10 or app-provider acceptance.

The main source/CPU window was released after that terminal result. The live
owner-authorized Test endpoint was not changed. The original 480df4526514e294f5a3a16dd4ab83a1fbcc7f47
U10-U11 coherent batch gate remains pending. Backup/restore, publishing and
production deployment remain outside this delivery.
