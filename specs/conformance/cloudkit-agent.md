# Signed CloudKit device agent

Spec: conformance/cloudkit-agent
Requirements: R2047, R2048, R2049, R2050, R2051, R2052, R2053, R2054, R2055, R2056, R2057, R2058, R2059, R2064, R2065; supports R2066
Anki: none (Meguri design)
Scenarios: none (protocol tests, app-hosted `CloudKitSoakTests`, and signed runs of conformance/cloudkit-acceptance Q7)
Status: Under review (R2066) for J4.1, J5.1 and two 2026-11-10 pointer sentences: "J4.1 states how the journal shows that order." in J4's engine-observation paragraph, and "After a relaunch, J5.1 states which process the host joins and which work that `close` joins." at the end of J3.2's "Original exchanges and peer evidence". Do not implement them until approved. Every other section, and every other sentence of J3.2 and J4, is in force and unchanged.

This file defines the command file and durable journal of `conformance/cloudkit-acceptance#Q7.3`. It supplies a protocol for the executable agent, not evidence that an agent or a signed run exists. Q4, Q5 and Q7 of that contract remain authoritative. This file adds no sync deadline, retry, account permission or acceptance exception.

## J1. Executable boundary

`tools/mg cloudkit-signed` is the host coordinator. `CloudKitSoakTests` runs inside each signed Debug Meguri app in the test VM. It opens the device's generated collection through the real core, uses the real `SyncCoordinator` and `CloudKitSyncAdapter`, and creates the real CKSyncEngine. A fake factory, standalone process pretending to be the app, or receipt writer cannot supply signed observations.

The first command is Q4.3's `launch`. The app-hosted test writes its durable `started` before it opens `CoreHost` or starts the coordinator and engine. Startup events belong to that command. J2's product and container checks precede account lookup; full account binding precedes database operations. No normal app startup may enable sync before this boundary.

The coordinator delivers commands to the app's run directory and copies its journal back. It preserves the exact delivered and returned bytes. The app remains the journal's sole writer. A relaunch opens the same device data and run directory. It does not create a fresh collection or discard earlier progress.

Data comes only from Q4.2's generated bases. Commands use Q4.3's steps that real devices can run and the selected Q7.5 leg's test hooks. Commands cannot run shell commands, source code, SQL or paths supplied as text. CKSyncEngine keeps scheduling its own work between commands.

Release builds exclude the agent entry point and its test hooks, including database cleanup. The cleanup call independently enforces Q7.2's development-container guard before deleting anything. A development suffix, a build receipt or a command field cannot enable that call in Release.

## J2. Immutable run header

The device's run directory holds `run.json`, one UTF-8 JSON object. Before any commands, the coordinator saves this header to durable storage. The header stays unchanged for the run. Before accepting commands, the app checks its structure and source, product and container fields. Runtime account checks follow the startup order below.

| Field | Meaning |
|---|---|
| `protocol_version` | Integer `1` |
| `job`, `run_id`, `lane` | Exact MG job `cloudkit-signed`, enclosing MG run id, and `cloud-signed` |
| `source_commit`, `tree_clean` | Exact source commit; `tree_clean` must be `true` |
| `device`, `device_kind`, `os_build` | Run-local device label, Mac/iPhone/iPad kind, and actual OS build |
| `app_build`, `executable_sha256` | App build identity and lower-case SHA-256 of the running bundle's `CFBundleExecutable` bytes |
| `build_receipt`, `build_receipt_sha256`, `deployment_receipt` | Exact MG build and device-deployment receipt references (J2.1), plus SHA-256 of the build receipt's bytes |
| `container`, `environment` | Verified development container identifier and development environment |
| `account_scope` | Run-local opaque label for the approved test-account scope |
| `account_labels` | Test-account labels needed by the selected commands: `acct1`, and `acct2` only where Q7 requires it |
| `selection` | `smoke`, or one exact Q7.5 leg id |
| `fixture_delivery` | The generated source and frozen input admitted for this device's initial launch (J2.3) |

The runner checks source and build fields against the actual signed product and its build receipt. The agent checks its actual product, OS, cloud configuration and runtime account binding against that expectation. These fields are expectations to verify, not declarations to trust. Ad-hoc signing or missing CloudKit grants cannot prove signed execution.

`account_scope` names the run's verified test-account scope, without account identity. Completed Q7.2 setup supplies the expected test accounts through protected setup data kept private to this run. The core checks the actual current identity against that scope and its binding. Logs contain only the fixed account label and check result. The label alone, a copied label or a cached identity is insufficient. Missing setup or expected account data awaits Q7.2's user step. A different account, container or environment fails with `wrong-container`. Export no account identity, its digest or other value that could identify the same account across runs.

Check the running product, container and environment first, with no CloudKit calls. Once those checks pass, durably record `launch` as `started` before opening the local core. The agent may then read account status and current identity without writing anything. The core must compare that actual identity with the protected expected test-account scope; a cached identity cannot satisfy this check. Until the account matches, permit no CloudKit database fetch, save, zone operation or cleanup. A missing or failed match leaves all those operations disabled. Account setup remains the user's work.

Account-changing legs retain the original header and scope. They observe only their planned transitions between the approved accounts or to no account. They never relabel an unknown identity as `acct1` or `acct2`. Signing in, signing out and account setup remain the user's actions.

`header_sha256` means SHA-256 of the exact `run.json` bytes. Every command and journal record carries it. A new MG run, product, device or account scope needs a new header and directory. A resumed app uses the original ones. Copying another run's journal never changes these bindings.

#### Device-local run directory

For Mac, iPhone and iPad, use exactly
`Meguri/CloudKitSigned/<run_id>/<device>/` below the executing development
app's OS Application Support directory. Obtain that root from the app's OS
container API; canonicalize the trusted OS root, then append the fixed path
and validated run/device labels. Do not canonicalize an untrusted candidate
into acceptance. `MEGURI_CLOUD_AGENT_RUN_DIRECTORY` is an expected absolute
path, not permission to select a root. Before opening any run file, require
exact equality with the derived path. Reject aliases, parent escapes, invalid
labels and another app's container. Then match the header's existing run and
device fields to those path labels. No new header fields are needed.

The coordinator resolves this location inside the frozen destination VM.
For iPhone/iPad, use the reserved simulator's actual OS `get_app_container`
data-container result, not its app-bundle path. The executing app independently
confirms its own Application Support URL. Neither a host HOME value nor a
header-supplied pathname establishes the device container.

For Mac, use a bounded automatic app-hosted path preflight in the verified
VM product, or an equivalent OS observation of that same installed app's
container. A path string does not prove this observation. The
setup test reads its OS Application Support URL and checks the loaded app
and test images. Use a low-level loaded-image check
that does not depend on the four delivered product receipts: their destination
is what this preflight is resolving. `AgentRunningProductBinding`, which needs
those copies, cannot supply this earlier observation. The executing app must
confirm the exact URL before accepting its run directory.

Select this setup-only entry path before normal App initialization can open
Core or start the collection/coordinator. The preflight performs no account
lookup, Core/collection open, collection creation or CloudKit call. It may
obtain/create the normal Application Support directory through the OS API.
Its result is only path preparation, never a command, journal outcome, signed
execution or Q7 pass. It introduces no user setup step. Keep the full J2.1
product checks before the real agent's account and database work.

Place `run.json` and all four `product-evidence/` files at their existing
relative paths below this root. J2.3 fixture roles, installed `data/` and J3's
command/journal paths use that same root without remapping. The device's
J2.2 private input remains outside it, at that section's separate relative
path below the same OS Application Support root. Never copy private account
input or routing data into the run's public evidence roles.

Bind this root to the original header bytes, actual device/VM instance and
retained filesystem identities. Check these at every handoff, delivery and
relaunch. Reject a different app container or root. Never move an existing
run or create another root for its header. Keep the actual OS, copy and
readback observations under the delivery rules; a copy-success Boolean proves neither
the root nor its use by the agent. Durable initial `launch/started` and J2.3's
verified one-time installation still precede opening Core.

### J2.1 Executable and receipt binding

`executable_sha256` hashes only the executable file, never an undefined whole-bundle encoding. It has the meaning of the existing MG `standalone_product.executable_sha256` field. The producer must provide this check for Mac, iPhone and iPad products; a Mac preview result cannot stand in for another device's product.

`build_receipt` and `deployment_receipt` each contain `{job, run_id}`. Resolve the exact MG receipts and only their declared artifacts using Q13.1's path and envelope rules. Require the run's clean source commit and the actual device/product binding. The coordinator independently checks the product signature, required CloudKit and push grants, provisioning under the existing platform rules, Debug/test-hook configuration, development container and environment, and the product actually delivered to this device. The running executable's bytes must match `executable_sha256` and that checked delivery. An executable hash alone proves none of the signing or account conditions. A caller-written receipt or a standalone preview's product details cannot substitute for these checks.

Q7.2 and Q7.3 require the Mac app and the reserved iPhone and iPad **simulators
in the test VM**, using real CloudKit and approved test accounts. Physical iOS
devices are not a new requirement. A generic simulator compile, a host-screen
run or a fake engine supplies none of the required signed execution evidence.
The header keeps `device_kind` values `mac`, `iphone`, `ipad`.

#### Build identity

`app_build` is exactly the nonempty string `CFBundleVersion` from the actual
app's Info.plist. It is neither the marketing version nor a concatenation with
the OS, source commit or configuration. Reject another type or value.

The build receipt is an actual passing `build-app` MG receipt for scheme
`Meguri`, configuration `Debug`, explicit test plan `CloudAgent`, and platform
`mac` or `ios-sim`. The public build command must explicitly select that plan;
an environment variable that only selects test execution does not select it.
`CloudAgent` must select the app-hosted `MeguriSnapshotTests/CloudKitSoakTests`
target from this source commit. A normal UI build, standalone preview, package
test, Profile, Release or AppStore product cannot substitute.

The receipt declares the actual products directory and one JSON artifact
`agent-build.json`. `details.agent_build` contains its exact
`{path, sha256, bytes}` reference. Thus J2's build-receipt hash also binds the
manifest bytes, not merely a mutable artifact pathname. Its object contains exactly:

| Field | Value |
|---|---|
| `protocol_version` | integer `1` |
| `job`, `run_id`, `source_commit` | the enclosing build receipt's identity and clean commit |
| `scheme`, `configuration`, `test_plan`, `platform`, `signing` | `Meguri`, `Debug`, `CloudAgent`, `mac` or `ios-sim`, and `team` |
| `app_build` | the actual app CFBundleVersion string |
| `app`, `agent` | each `{path, bundle_id, executable, executable_sha256}`; paths are products-relative; executable is relative to its bundle |
| `xctestrun` | `{path, sha256, bytes}` for the actual CloudAgent xctestrun, relative to products |
| `products` | `{path, entries}`: the declared products root and complete frozen input inventory described below |

`app` identifies the real development Meguri host. `agent` identifies the actual
app-hosted MeguriSnapshotTests bundle, with its built Info identity and binary.
The coordinator checks both signatures and the actual app's existing platform
grants, configuration, development container and environment. It checks the
resolved build settings against the actual products, not producer Booleans.
For iOS simulation, verify that the built binary targets the simulator OS and
supports the selected device class. Use the existing team-signing policy,
development entitlements and schema-readiness checks. This requires no profile
for physical hardware and no xcdevice result. An ad-hoc build remains compile-only
evidence. On Mac, verify development signing and profile permission for the
chosen test VM under the existing rules.

`entries` maps every relative input path to one exact entry:

- file: `{type: "file", sha256, bytes, mode}`;
- directory: `{type: "directory", mode}`;
- symbolic link: `{type: "symlink", target}`.

Modes are integer POSIX permission bits, without timestamps, owners or inode
numbers. Inventory the host app, its debug dylib when present, the agent bundle,
their product dependencies/resources, and the selected xctestrun. Include every
input the selected xctestrun or its environment uses to load Meguri/agent code.
Do not silently omit a dependency or bind only the main executable. The exact
file set and actual bytes must match; extra test code is not harmless metadata.
Links may describe normal internal bundle links only: relative targets must
remain inside this frozen product root and resolve to an inventoried entry.
Hash terminal files through their direct physical paths without following
links. Reject external links, missing targets, link cycles, parent escapes,
special files and ambiguous paths. Use J2.1/Q13.1 stable no-follow authority
checks around the inventory and independent signature inspection.

The producer freezes this manifest only after the final build and signing
steps. No later re-sign, widget recopy or test-bundle substitution may keep the
same proof. A required product transformation needs a new real build receipt.
The actual source commit and clean-tree state must remain the same through
build completion and freezing; recording a clean value only at job start is
insufficient.

#### Intended destination

The coordinator assigns run-local device labels once, using the configured
test VM and Q7.2's reserved pool devices. Before delivery it freezes a private
`destinations.json` below host Application Support at
`Meguri/CloudKitSignedPrivate/<run_id>/destinations.json`. Apply J2.2's private
owner/mode, strict JSON and stable no-follow rules; do not export this file or
its digest. This is coordinator-selected routing, not account self-enrollment.

Use exactly `protocol_version`, `job`, `run_id`, `source_commit`, `destinations`
in this object. Version is `1`. Match job, run and source to the parent
`cloudkit-signed` run. Each destination label has exactly
`{device_kind, vm, vm_instance, simulator}`. Select `vm` from the configured
test VMs; it cannot contain an SSH command or a path. Read `vm_instance` from
that VM's actual instance. Set `simulator` to `null` on Mac. For iOS simulation,
use `{udid, lease_receipt}` and identify the real MG pool acquire or renew run.
Match the lease to Q7.2's selected device, its type and the owning checkout.
Only the checked VM and live pool supply routing. Keep those values private;
the run header cannot choose them. Writing this map is automatic coordinator
work, with no added user step.

Reuse the approved private account data from completed Q7.2 setup for later
runs. The coordinator copies those unchanged values and applies each new
header binding under J2.2 automatically. This creates no repeated user step.
A current account lookup cannot supply the expected account. J2.2 defines
only the private approved-account input needed for automatic reuse, with no
new setup UI, password or token storage contract.

Hold the existing VM/pool lease while checking routing, copying, installing,
launching and reading proof. Verify the VM instance again, and for a simulator
verify the actual live UDID, kind, ownership/lease, booted runtime and OS build
with the pool's existing checks. A matching OS version or first available
device of that kind is insufficient. `header.os_build` equals this actual
destination's build. Changing the VM instance, lease target or device requires
a new run/header; do not silently route a relaunch elsewhere.

#### Actual delivery receipt

`deployment_receipt` names a real passing MG `cloudkit-agent-install` receipt.
That job first allocates its MG run id. The coordinator then freezes the header
that references the id. A pass covers actual delivery and its checks only;
signed scenario/soak cells remain unfilled. Its envelope uses the same clean
source commit. Declare `run.json`, `agent-deployment.json` and the actual bounded
copy/install observation artifacts used by the job.
`details.agent_deployment` is the exact `{path, sha256, bytes}` reference to
its manifest. The child job can freeze the parent header using its already
assigned deployment run id; the reference binds an id, not the future receipt
bytes. There is no circular hash of the header and final deployment receipt.

`agent-deployment.json` contains exactly `protocol_version`, `job`, `run_id`,
`source_commit`, `parent_run`, `device`, `device_kind`, `os_build`,
`header_sha256`, `build_receipt`, `build_receipt_sha256`, `build_manifest`,
`delivered_manifest`, `xctestrun`, `installation`.
Version is `1`; own job/run/source equal its envelope. `parent_run` is the
`cloudkit-signed` MG run id. Header/device/build fields equal the immutable
run header and exact resolved build receipt. Each manifest and xctestrun
reference is `{path, sha256, bytes}`, declared by the applicable receipt.

`delivered_manifest` has the same entry-map format as `products`, produced by
an actual no-follow inventory in the intended VM after transfer. Relative
bytes/entries equal the frozen build input; only its root path may differ.
Use the existing MG copy routines and verify the result, not tar/SSH exit status
alone. `installation` is `null` for the Mac; for a simulator it is a reference
to an actual MG/VM simctl installation observation for the frozen destination
and verified development app. A caller-written installed/success Boolean or
an inventory of attached devices is not delivery proof.

The coordinator launches `test-without-building` with that exact verified
xctestrun, explicit CloudAgent selection and frozen destination. Preserve its
original xctestrun bytes. Record run-specific launch environment separately;
allow only the existing fixed agent-run-directory input and normal MG/Xcode
test variables, never arbitrary code paths or injected libraries. Any necessary
derived xctestrun must have its exact bytes declared in delivery, retain the
build template hash, and restrict changes to those documented routing/run-path
fields. No test target, binary, test selection or checker policy may change.

Before the first CloudKit call, the agent observes its actual OS/build, host
executable and loaded test-bundle identity and bytes, and matches this verified
build/delivery. A bundle found at a requested path alone is not proof that it
is the bundle executing the agent. The coordinator retains the original launch
and app-hosted test observations with the real agent journal. Recheck frozen
input authority before a relaunch and preserve the original product and device.
Copy/install proof, successful launch, a bootstrap check and a smoke each remain
only their own scope; none supplies Q7.4's whole-lane outcome.

#### Required refusal cases

Reject a UI/standalone build as CloudAgent proof; CFBundleVersion substitutions;
stale agent or debug-dylib bytes with an unchanged main executable; a changed
xctestrun target/selection/environment; wrong live VM or simulator with a
matching OS; expired/wrong-kind/wrong-checkout lease; recreated VM under the
same name; missing post-copy inventory; claimed install without actual
observation; changed product after signing/copy; external product links; and
source/header/build/deployment reference substitutions. Fake inspectors remain
unit-test evidence only. Missing producer, transport, inspector or loaded-agent
binding code fails `implementation_missing`, not `awaiting_user`.

#### Product evidence delivered to the agent

Before launching the test, the parent coordinator copies the exact finalized
build receipt and manifest and the exact finalized deployment receipt and
manifest into this agent's run directory. Fixed relative paths are
`product-evidence/build-result.json`, `product-evidence/agent-build.json`,
`product-evidence/deployment-result.json`, and
`product-evidence/agent-deployment.json`. No credentials, expected-account file
or destination mapping enters this directory. Stable no-follow reads and
strict duplicate-key rejection apply to every copy.

The original receipt bytes remain unchanged, including their host artifact
paths. These four fixed copies are a transport of those exact bytes, not new
receipts. Verify the header's build receipt hash, exact receipt ids, source,
outcomes and manifest references/hashes before using them. Resolve a copied
manifest only by its fixed role above after verifying its original declaration
and bound digest; never treat an arbitrary host path from JSON as a device path.
These checks bind observations of the running app and agent to the source
manifest. The host must verify what it actually copied and where it went.
Evidence copied from another product or destination does not pass these checks.

### J2.2 Protected expected accounts

The host reads `Meguri/CloudKitSignedPrivate/<run_id>/<device>/expected-accounts.json` under the current user's Application Support directory. The device reads the same relative path under its app container's Application Support directory. Keep both copies outside the checkout, `build/`, fixture directories and public MG artifacts. The coordinator copies only the account choice supplied by approved Q7.2 setup, with this run and device binding. Reading the current account cannot create or change the expected account.

The private directories have mode `0700`. The input is a regular file with mode `0600`, one link, and the process user's ownership. Check that owner on the private directories too. Read through stable no-follow file and directory bindings; reject links, paths writable by another user, changed file bytes and changed path authority. These checks start at the admitted Application Support root, following the existing protected-input pattern. The host and device copies must preserve these restrictions.

The JSON object contains exactly `protocol_version`, `header_sha256`, `job`, `run_id`, `source_commit`, `device`, `executable_sha256`, `container`, `environment`, `account_scope` and `accounts`. The version is `1`; every binding field equals J2's header. `accounts` maps exactly the header's account labels to the actual expected CloudKit user-record identities supplied by completed Q7.2 setup. Each value is a nonempty array of integers from 0 through 255: the exact opaque identity bytes the core uses under `sync/cloudkit-recovery#V9.1`, with no case conversion or hashing. Parse strictly, including duplicate-key rejection. Compare the actual core-verified current identity with this private input before database work, using J2's startup order.

Examples, commands, journals and public artifacts contain no account IDs. Keep the private input and its digest out of exports. Do not export an account hash or another value that identifies the same account between runs. Report failures with fixed labels and check results only. If the user has not supplied the approved expected accounts, await Q7.2 setup. A current identity that differs fails `wrong-container`. Missing code to read, deliver or compare the input fails under J6; it is not a user setup step.

#### Reusable approved source

The host reads `Meguri/CloudKitSignedPrivate/approved-accounts.json` below
its own OS Application Support directory. This is the reusable input from
completed, user-approved Q7.2 setup. It contains only already approved identity
bytes. A current account read alone never approves, creates or updates it.
Only the owner-triggered procedure in J2.2.1 can publish those values after
explicit approval. The coordinator treats the source as read-only. The device
reads only its bound per-run input, never this host source.

Apply the private owner/mode, single-link, strict JSON and stable no-follow
rules above to this source and its directories. Its object contains exactly
`protocol_version`, `container`, `environment` and `accounts`. Version is `1`;
environment is `development`; container equals the independently verified
development product. `accounts` has `acct1` and, once approved, `acct2`, with
exact nonempty identity byte arrays in the existing J2.2 encoding. Reject
unknown labels and equal identities for the two different test accounts.
A missing label cannot support a leg that requires it.

For each new run/device, project the required approved labels unchanged into
the existing per-run input and fill its existing header binding fields. This
is automatic coordinator work, not a request to rewrite that file by hand.
Pin and recheck the approved source while materializing the per-run copies.
Changed source cannot retarget an existing run; different approved setup needs
a new run/header. Keep all identity bytes and their digests out of public logs
and artifacts. Missing approved setup data awaits `cloudkit-test-accounts`;
missing source-reader, materializer or path-preflight code fails
`implementation_missing`.

#### Expected scope for one run

J2/J2.2 require stable private input admission and a fresh actual account match
before CloudKit database work. They do not define the private input file as a
per-network-start revocation switch for CKSyncEngine's autonomous operations.
Keep the approved expected-account values fixed for the run. A change to that
scope requires a new header and directory. Never substitute other expected
values within this run.

Revalidate retained private authority at agent delivery/command admission,
before and after an awaited fresh account read, and at app/Core operation
admission already owned by this test agent. A change before the account match
refuses all CloudKit database work. A later observed change fails the run,
revokes future agent admission and cancels/joins owned work under J5. Original
terminal callbacks may drain as J5 permits; they are not another successful
operation or authority to start a successor. Preserve Core's actual identity,
binding, SDK and batch guards and the same guarded factory for all account
reads. Keep CKSyncEngine automatic scheduling. Do not claim to intercept each
internal SDK network start. An app may also check its native transport boundary;
that does not add a requirement to inspect each packet.

### J2.2.1 Owner-triggered test-account setup

`tools/mg cloudkit-account-setup --label acct1` prepares J2.2's reusable source.
Use `acct2` only after `acct1` exists. The utility belongs to Q7.2's existing
`cloudkit-test-accounts` user step. Only Antoine invokes it and enters approval.
Agents may prepare its products and instructions; gates and catch-up never run
it. A dry run performs no lookup or source write.

Before invoking it, the owner confirms the intended dedicated test account in
the VM's account settings. The utility must not guess the account from a label,
an email address, a cached identity or the host's account.

#### Product and execution boundary

Use the dedicated VM Mac destination and retain its exclusive lease. Reuse the
real Debug `CloudAgent` build, install and private transport machinery. Apply
J2.1's actual signature, provisioning, source, loaded-product, container,
environment and schema-readiness checks before account lookup. A compiled
product, development suffix or caller Boolean is insufficient. Do not change
readiness flags or infer that the owner configured the portal or schema.

Select a separate setup-only app-hosted entry before normal App initialization.
It opens no collection, Core host, coordinator or engine. It performs only native
account-status and identity lookup, using V9.1's exact identity encoding. It
performs no zone, record, asset, cleanup or other database operation.
Release and AppStore builds exclude this entry and its startup selector.

This producer needs no approved account input for its read-only candidate lookup.
It cannot select the normal agent entry or bypass that entry's expected-account
checks. A candidate is not approval, a Core identity proof or signed-lane evidence.

#### One invocation and one deliberate approval

The invocation accepts only the fixed label `acct1` or `acct2`; it accepts no
identity, container, environment, account hash or candidate path from the caller.
The verified development product supplies the container and environment.

1. Admit and pin the existing approved source, if present, under J2.2's rules.
   Refuse a malformed source or another container or environment. `acct2` needs
   an existing approved `acct1`.
2. Obtain a fresh available native identity in the verified app. Retain that
   candidate privately, with its original product, VM and container authority.
3. Require input from Antoine's interactive terminal: `approve acct1` or
   `approve acct2`. If this changes an existing approved value, require
   `replace acct1` or `replace acct2`. Display only the requested label,
   verified development container and whether the stored entry changes.
4. After that entry, perform another fresh read in the same original app process.
   Require available status and exactly the candidate's identity bytes. Recheck
   the original product, VM, container, lease and source authority.
5. Publish only the approved label's value. Preserve every other approved
   label unchanged. Require distinct identity bytes for `acct1` and `acct2`.

Piped input, a default, an environment variable, a flag or process ownership
cannot supply approval. The terminal is an interaction channel, not independent
proof that an arbitrary caller is Antoine. R1969's owner-only rule still applies.
Refusal, EOF, cancellation or a failed recheck before publication leaves the source
unchanged. Do not repeat a lookup or approval automatically after failure.

Candidate bytes stay in process or private owned file descriptors during this
invocation. Use the existing private transport; keep no candidate for later
invocations and no public capture artifact. If the original app or transport ends before
publication, discard the candidate. A new invocation needs new capture and approval.

#### Private publication and outcome

Publish at the host's OS-derived Application Support location already fixed in
J2.2. Canonicalize the trusted OS root, then admit every private component through
stable no-follow authority. Keep directories `0700` and regular files `0600`,
current-user-owned and single-link. Reject substituted or linked components.

The source retains exactly `protocol_version`, `container`, `environment` and
`accounts`, version `1`, with J2.2's exact byte arrays and strict JSON rules.
Write a new private temporary file in the same admitted directory. Flush it,
recheck the original source and directory bindings, then publish atomically.
Flush the directory and verify the published bytes through retained authority.
Never modify an existing source through a stale file descriptor or silently
replace a source that changed during approval.

Once atomic publication starts, cancellation or a lost completion can leave its
outcome uncertain. Report failure and retain private evidence; do not report an
unchanged source, undo the approved write or repeat it automatically. A later
owner invocation must inspect the actual source before deciding another update.

The utility exports only fixed outcomes and account labels. It exports no
identity, identity digest, candidate bytes, private path or raw SDK error.
Keep private transport and SDK diagnostics out of logs, XCTest attachments,
result exports and copied run artifacts. Cancel and join original work before
releasing the lease or finalizing the outcome.

| Observed outcome | Required result |
|---|---|
| Explicit approval, same fresh identity, valid atomic publication | Setup-only success; the source contains the approved value |
| Owner declines, EOF or cancellation before publication | No approval and no source change; join original work |
| Missing required owner setup or no signed-in test account | Existing `awaiting_user` disposition and matching Q7.2 step |
| Wrong product, changed authority or identity, or invalid source before publication | Failure; preserve the source and fixed refusal category |
| Publication completion is uncertain | Failure with private evidence; no automatic rollback or repeat |
| Missing producer or required verification code | `implementation_missing`, never a user-setup or passing result |

Machine unavailability follows the existing MG placement rules. An attempted
operation that fails is not an unrun queued check. An account read error supplies
no identity proof and cannot approve a value or trigger an automatic repeat.

Setup success fills no `cloud-signed`, leg, soak or release scope cell. It does
not complete all Q7.2 account steps: required device sign-ins and later `acct2`
setup still need their own evidence. A changed approved value needs a new
normal run and header; existing run input never changes.

#### Focused producer checks

Generated tests must cover valid creation, idempotent approval, explicit
replacement, and preservation of the other label. They must also cover:

- No lookup before failed product or container admission; no Core or database work.
- No publication from a candidate alone, piped/default approval, refusal or EOF.
- Changed identity, product, VM, lease or source during approval; no source change.
- Invalid private modes, owners, links, duplicate keys, byte types and equal accounts.
- Interrupted publication, original-process loss and cancellation; original joins.
- Private markers in SDK failures and candidate data; no public value or digest.
- Setup-only success never produces normal-agent or signed-lane acceptance evidence.

Generated checks prove the producer's refusals and ownership only. Actual owner
setup still requires the verified signed product and the deliberate approval above.

### J2.3 Initial generated data

`fixture_delivery` contains `base`, `fixtures_receipt`, `source`, `manifest`, `frozen_input` and, only when preparation is needed, `preparation_receipt`. Receipt references are `{job, run_id}` under J2.1's MG resolution rules. `fixtures_receipt` identifies the real passing MG `fixtures` run. `source` is its declared generated fixture artifact. `manifest` and `frozen_input` each contain `path`, `sha256` and `bytes`. The manifest path is relative to `source`. The frozen-input path is exactly `fixture-input.json` in this device's run directory, outside the fixture source. Resolve neither through links or a parent-path escape. They identify exact input files, not a later export from the app.

`base` names the Q4.2 base the selected work requires. The initial producer may implement only `small`; it must not claim that other bases or a complete leg ran. The MG `fixtures` target for that base is `cloud-small`, not a literal `small` argument. The producer needs the real generated request and, only when necessary, an actual preparation run. Missing that producer or its checks is implementation work, never proof supplied by a `generated` marker.

For `small`, verify the actual output against Q4.2's marked deck roles, pinned
clock, 20 single-card notes, five distinct media references and 60 history rows
over its three specified days. Retain the real request and preparation proof;
the base label or requested counts alone do not establish those file and row
facts. The original generator manifest and any later preparation remain
separate under the frozen-input rules below.

The frozen JSON input contains exactly `protocol_version`, `run_id`, `device`, `source_commit`, `base`, `parent_files`, `prepared_source` and `files`. The version is `1`; run, device, source and base match the header. `parent_files` maps every source-relative parent file path to `{sha256, bytes}`. `prepared_source` is either that same source or the prepared fixture artifact declared by `preparation_receipt`. `files` is the corresponding exact source-relative file map for the data to install. Both maps include the database, manifest, any required WAL and every media file. Check exact file sets and actual bytes, not only declared digests. The real generator request, manifest and any preparation evidence must establish the generated base and its recorded changes. Preserve the parent's manifest; do not claim its digest describes prepared database changes.

Freeze those inputs before launch. The header binds the frozen input's bytes; its run/device/source fields bind it back without a circular header hash. The full maps and their generated media names are fixture input, never text to copy into logs or signed evidence. Public observations use fixed file labels, sizes and digests only. A returned collection or later app state cannot become the expected source.

#### Fixed device-local fixture roles

`source` and `prepared_source` retain their original host artifact paths as
receipt bindings only. The device never opens those paths or treats them as
local destinations. Use these fixed roles below the device's run-owned directory inside the development app container:

| Role | Fixed relative path |
|---|---|
| Original referenced MG fixtures receipt bytes | `fixture-evidence/fixtures-result.json` |
| Selected original generation JSON bytes | `fixture-evidence/generation.json` |
| Original generated source directory copy | `fixture-evidence/parent/` |
| Optional referenced preparation receipt bytes | `fixture-evidence/preparation-result.json` |
| Optional prepared source directory copy | `fixture-evidence/prepared/` |
| Existing frozen input | `fixture-input.json` |
| Collection installed once for this run | `data/` |

The original manifest is read only at `fixture-evidence/parent/` plus the
header's source-relative `manifest.path`. Match its exact bytes, size and digest
to the header. Preserve it separately from any prepared manifest/database.
For a direct `small` delivery with no preparation, require
`prepared_source == source` and exact map equality `parent_files == files`.
Use the parent copy directly; the optional receipt and prepared-copy roles must
be absent. Do not create a link or duplicate prepared directory for this case.

When preparation is present, its receipt must match `preparation_receipt` and
declare the `prepared_source` artifact. Validate the actual preparation and
readback evidence required by that producer, with the original parent binding.
The parent copy still matches `parent_files`; the prepared copy matches `files`
and supplies the installation bytes. Missing knowledge of the actual preparer
or its proof fails `implementation_missing`; a passing envelope alone proves
no preparation. This clarification does not define a new preparation format.

Apply J2.1/Q13.1 exact reference, complete-envelope, clean-source, passing-result
and artifact-declaration checks to the copied original receipts. A copied
receipt remains its original bytes, not a new receipt with local paths. Require
the header's exact fixtures/preparation job and run ids and source commit.
`source` must be the declared original fixture artifact; the manifest and
selected generation output must be declared by that same fixtures run.
Interpret each fixed local role only after its original declaration matches.
No JSON pathname selects another local file or substitutes another receipt.

For `base: small`, select the actual `cloud-small` generation from the passing
MG `fixtures` receipt. Exactly one `details.fixtures` entry must have target and
kind `cloud-small`. Its `request`, `manifest` and `observed` values must equal
those three values in the declared original generation JSON. Compare this
projection, not the complete MG entry: its added `target` and `kind` metadata
are not generation-output fields. Require the real request to satisfy Q4.2:
seed, pinned clock, UTC/rollover, 20 notes/cards, five 10,240-byte media files,
three Again answers per card on the three fixed days, and ordinal answer times.
The generation manifest must match the persisted parent manifest. The observed
counts and logical dump digest must match that manifest. A `generated` flag,
requested counts or the word `cloud-small` alone supplies none of this proof.

Check the complete parent/prepared file sets and their actual SHA-256/byte
counts against the frozen maps, including the database, manifest, required WAL
and every media file. Match the exact frozen-input bytes to the header's
`frozen_input` reference, then its run/device/source/base fields to the header.
Retain stable no-follow file/directory authority, ownership and no-write-by-other
checks. Reject links, path escapes, duplicate/aliased paths, special files,
missing entries, extra entries or changed bytes. These original receipt copies,
maps and generated contents are fixture inputs, never public log text.

After durable initial `launch/started`, validate these inputs, require an absent
run-owned `data/`, install its selected copy once, and verify the installed
bytes before opening Core. Never use the default/daily-driver collection.
On relaunch, open the existing run-owned data and tracking without installation.
Keep the original fixture and frozen maps unchanged. Modified live data and
later exports cannot become those inputs. After a kill during the first install,
check the actual state against the planned recovery. That recovery code must
exist and pass its checks before continuation. Do not delete the destination or
copy the source again to obtain a passing result. J4 operation proof and Q5
checks remain required.

#### Fixture-delivery refusals

- Host-only or substituted source/prepared path used as a device pathname.
- Wrong original receipt/job/run/source/outcome or undeclared artifact.
- Missing, duplicate or mismatched selected cloud-small generation/request/readback.
- Parent manifest replaced by a prepared manifest, or unchanged manifest claimed
  as proof of a changed prepared database.
- No-preparation delivery with unequal sources/maps or unexpected prepared roles.
- Missing/extra/aliased/linked files, changed bytes, invalid authority or wrong
  frozen-input/header binding.
- Existing initial destination, default collection target, or Core opened before
  durable start and verified installation.
- Relaunch that repeats installation, adopts changed live data as fixture input,
  or deletes a partial installation instead of proving recovery.

## J3. Ordered command delivery

`commands.jsonl` is UTF-8 JSON Lines. Each complete line is one command object. The coordinator appends commands durably; it never edits or removes an earlier line. The agent accepts complete lines only. An incomplete final line is not a command and causes no effect. The coordinator may finish that line; it cannot change an accepted complete line.

| Field | Meaning |
|---|---|
| `header_sha256` | Exact J2 header digest |
| `command_seq` | Consecutive integer, starting at `1` for this device |
| `leg` | Header selection, or `smoke` |
| `action` | Q4.3 step name, or a test-hook operation required by the selected Q7.5 leg |
| `arguments` | Typed step arguments, using generated-fixture references and run-local entity labels |
| `account_before`, `account_after` | Expected approved account label, or `noAccount` at a planned account boundary |
| `recovery_check` | Reference to the selected leg's expected recovery check if this command may be interrupted; otherwise absent |

`command_sha256` means SHA-256 of the exact command line bytes, excluding its final LF. Arguments resolve to generated data inside the agent; exported commands contain no field text, deck names, tags, media names or media bytes. Unknown operations, malformed arguments and operations outside the selected leg fail before execution. The protocol does not grant additional recovery or destructive actions.

The coordinator delivers at most one command without a terminal result per device. It checks that result before delivering the next command. It also enforces the selected scenario's cross-device order from the returned evidence. Command completion can mean that a specified engine event occurred while an operation remains in flight; it must not falsely report that operation complete. This permits the concurrency that Q7's engine checks require.

The agent validates the complete accepted prefix on every delivery and relaunch. Sequence numbers must be consecutive; each journaled command must retain its exact digest. The next new command must be the next sequence number. A gap, reorder, changed earlier command or binding mismatch fails without executing the new command.

The coordinator durably retains each returned complete journal prefix before delivering more work. Every later journal must extend those exact bytes, including after relaunch. On reconnect, the agent checks its local journal against that retained prefix before opening the core or starting sync. A lost or changed previously returned record fails; the coordinator cannot treat a shorter journal as a fresh run or replace it with a copied success. This uses the journal bytes already retained as artifacts, without a separate progress database or digest chain.

Repeated delivery of identical accepted commands causes no repeated operation. A completed command retains its existing result. An incomplete command enters J5 recovery. A failed command stays failed. The coordinator does not rewrite a command, advance past failure or automatically run it again under another sequence number.

### J3.1 First generated-data smoke

For selection `smoke` with direct base `small`, the first Mac workflow is the
following exact sequence. Use `acct1` for both account fields of each command.
These are J3 command arguments, not native IDs or content. Require exactly the
listed keys, types and literals; reject other keys or values. An empty object
has no keys. This form adds no header field or protocol version.

| Action | Exact `arguments` object |
|---|---|
| `launch` | `{}` |
| `connect` | `{"preview":"p1"}` |
| `publish` | `{"preview":"p1"}` |
| `add_note` | `{"note":"n1","deck":"D2"}` |
| `start_session` | `{"session":"s1","deck":"D2","card":"c1"}` |
| `answer` | `{"session":"s1","card":"c1","rating":"good"}` |
| `end_session` | `{"session":"s1"}` |
| `add_media` | `{"media":"m1"}` |
| `sync_now` | `{}` |
| `close` | `{}` |

Labels `p1`, `n1`, `s1`, `c1` and `m1` belong only to this run. The agent maps
them to actual preview, note, session, current card and stored media facts.
`D2` is Q4.2's logical child-deck role, not its actual marked name. A command
cannot provide a native identifier, path, field text, file name, bytes, SQL,
code, account choice, requested result or replacement override. This first
producer supports no kill/relaunch or other selected leg; missing recovery
cannot be claimed as covered. The existing failure and no-repeat rules apply.

#### Startup and first publication

`launch` performs J2's actual product/container checks, durable start,
J2.3's verified one-time install, protected fresh account match and real Core
open in the existing order. The generated collection starts without a cloud
binding. This local state is allowed. Do not create substitute tracking records
or require a prior signed-lane pass. Record actual initial local integrity
under the check below. No engine or publication starts as a side effect of
this local open. Guarded read-only account lookup and V13 preview work remain
permitted in their specified order.

`connect` runs the real V13 connection flow outside a review session. It
waits for the actual account-bound root read and completed connection preview,
then retains that exact preview as `p1`. For this first publication, accept
only V13.3's existing-local-collection/no-active-epoch case, with the generated
local counts and no prior remote collection to replace. A remote epoch,
account mismatch, unreadable preview or another offered decision fails this
smoke without adopting, replacing, clearing or deleting remote data.

`publish` consumes `p1` once through V26.2's existing publication confirmation,
reason 1 and absent active change tag. Use the actual root read; creating a
replacement empty root or skipping current-preview/account checks is forbidden. A changed preview or a
concurrent publisher fails rather than choosing Replace again. Wait for the
real V15 publication to reach `done`, with its row removed, the actual binding
usable and its engine admitted under the existing gates. An enabled flag,
queued request or requested epoch is not completion. Existing publication,
root-confirmation, media and account guards remain unchanged.

#### Local review and media mutations

For `add_note`, resolve Q4.2's own `QA` type and `D2`. Generate the fields inside
the agent from the base marker `M`: `M-smoke-n1-Q` and `M-smoke-n1-A`, with no
tags. Use the normal add operation, flush, and read back the committed note
and its one card. Preserve its actual operation receipt and revisions under
`n1`; do not emit those field values or native IDs.

`start_session` opens the real review queue for `D2` and registers its actual
current card as `c1` in session `s1`. It does not force a card to the head or
change its scheduling state. An empty queue fails. `answer` reveals that card
and applies Good (rating 3) through the real review coordinator/session using
its current token, answer instant and elapsed-time seam. A stale/non-head or
failed answer fails; do not answer another card or retry. Wait for the actual
accepted answer to be committed and settled. Retain the card revision and
exact new durable review row; a count or optimistic acceptance is insufficient. `end_session`
finishes that same session through the normal path and observes its hold-back
release and pending-change handoff; it does not send a fabricated flush result.

For `add_media`, choose the desired name `M-smoke-m1.bin` in process. Its bytes
are the concatenation of 320 SHA-256 blocks, each hashing the UTF-8 string
`MeguriCloudSmoke/<run_id>/<device>/m1/<i>`, where `i` is decimal 0 through 319
without padding. This is one deterministic 10,240-byte generated test input,
not a new fixture base or evidence-signing format. Register it through the
normal media add service and use its returned stored name. Flush and verify
the actual physical file bytes and tracked media fact against that input.
Never overwrite a fixture file or export actual names/content. Retain the
actual media operation and resulting tracked revision under `m1`.

#### Required operation observations

Use J4's existing check entries and content-free observation records. Each
command's `done` requires its corresponding actual observations below; a
command cannot supply them as expected-success values. Fixed entity labels,
counts, sizes, canonical payload digests, typed statuses and references to
actual operation/engine observations are permitted; native IDs and content
remain private to the process.

| Action/check | Required actual facts |
|---|---|
| `launch.integrity` | coherent owned read: collection opens; full `integrity_check` is `ok`; exact U22 expected schema objects; no missing card-note/deck or note-type reference and no wrong note field count |
| `connect.preview` | `p1`, available approved account, generated local counts, absent active epoch and first-publication choice actually offered |
| `publish.completed` | `p1`, actual publication operation and its `done` transition, removed op row, usable binding and current engine instance; preserve actual SDK/own-operation order |
| `add_note.committed` | `n1`, real committed operation, one added note/card, durable readback and corresponding canonical digests/revisions |
| `start_session.head` | `s1`, `c1`, actual queue/head and its current state token checked in process |
| `answer.committed` | `s1`, `c1`, Good, actual accepted operation, settled durable card revision and exactly one distinct new committed review row with its canonical digest |
| `end_session.released` | actual end of `s1`, held answer released and real pending-work handoff |
| `add_media.committed` | `m1`, actual operation, physical and tracked byte count 10,240, equal SHA-256 and resulting tracked revision |
| `sync_now.result` | original coordinator-issued exchange, its actual terminal RecordExchange result/accounting, current pending/missing media state and revision-specific acknowledgement proof described below |
| `close.integrity` | the same full local integrity/schema/reference checks at a coherent safe final snapshot after owned work is joined |
| `close.joined` | actual coordinator cancellation/drain, owned work joined and Core closed, in that order; terminal journal remains durable/readable |

`sync_now` invokes the existing manual exchange once and observes that original
operation through its terminal result. Check the actual acknowledged current
revisions for `n1`, the answered card and new review row, the `m1` media-name
entity and its blob. Include actual per-record results/durable tracking and
blob-upload acknowledgement or verified existing-blob observation; neither
selected-count greater than zero nor a successful note readback proves these
facts. Earlier automatic acknowledgements may satisfy a current revision when
their actual operation/event and durable state are retained; do not falsely
attribute them to this manual exchange. A successful collection exchange may
still report pending media. Preserve that real result: until required media
proof exists, this smoke has no successful upload check. Observe existing work
within its current limits. Never reissue Sync Now or add another attempt.
This smoke proves no cross-device convergence. Unavailable required proof fails the smoke.

`close` is an explicit final command. Stop and join the actual coordinator's
owned work before the final integrity snapshot and Core close. Only then
record joined shutdown. The host retains that terminal journal and joins the
original app-hosted process; process exit or a lost connection alone is not
clean close. Failure follows J4 cancellation/join rules and starts no successor.

The local integrity observations must inspect actual storage/schema/references;
open/quick-check alone is insufficient. Q5's final-copy oracle clause applies
to its named Rust/property/fuzz/fault programs, not this Mac signed smoke. Run
Q5's applicable always checks throughout. This single-device smoke reports
only its observed operation coverage; it fills no Q7.5 leg, assumption ledger,
whole-lane/soak cell or Q13 first-sync/release acceptance. Required full-lane
and trial evidence remains outstanding, not a prerequisite for this first
SDK execution. Missing inspection or operation code is `implementation_missing`.

### J3.2 Selected two-device `sync_now` program

For header selection `sync_now`, use exactly one Mac source and one iPhone peer,
both on `acct1`. Both headers admit the same direct Q4.2 `small` generation under
J2.3, with separate device/run-owned data. Keep J2's actual Debug product,
development-container/environment and protected fresh account checks. This adds
no account setup, cleanup or remote-replacement permission. J3.1's smoke grammar
and ten positions remain unchanged. No new header field or protocol version is
needed. Missing implementation for this selection is `implementation_missing`.

Each device accepts only its following positions, with the exact argument keys,
types and literals shown. Both account fields are `acct1`. Labels are local to
the admitted run and device; `p1` never denotes the other device's preview.

| Position | Mac action and exact arguments | iPhone action and exact arguments |
|---|---|---|
| 1 | `launch` `{}` | `launch` `{}` |
| 2 | `connect` `{"preview":"p1"}` | `connect` `{"preview":"p1"}` |
| 3 | `publish` `{"preview":"p1"}` | `adopt` `{"preview":"p1"}` |
| 4 | `edit` `{"note":"n1","field":"Q"}` | `sync_now` `{}` |
| 5 | `sync_now` `{}` | `observe` `{"note":"n1"}` |
| 6 | `close` `{}` | `close` `{}` |

The coordinator first completes Mac positions 1–3, then iPhone positions 1–3,
then Mac positions 4–5, then iPhone positions 4–5. It compares the actual source
and peer observations before delivering either close. It delivers no successor
after failure. Reject other actions, arguments, positions, devices and account
labels before execution; commands supply no content, native IDs, paths, expected
digests, pass flags or replacement overrides.

#### Bootstrap and fixed edit

`launch` has J3.1's startup/local-integrity obligations. On the Mac, `connect` and
`publish` have J3.1's absent-active-epoch, actual preview and first-publication
obligations. An existing remote epoch or concurrent publisher fails this program;
never clear or replace remote data to make it proceed.

The iPhone's `connect` must observe that completed publication through its actual
account-bound root/epoch reads and ready preview. Its generated local collection
uses V13.3's existing-collection adoption choice. `adopt` consumes that exact
offered preview through the normal confirmation, backup, staging and activation
path. Prove the safety backup and actual completed adoption, with usable binding,
required scan and normal engine admission; do not seed binding/cursor/ACK rows or
publish a second epoch. Preserve product recovery ownership and media behavior.
For each device, derive canonical digests of the root and epoch payloads from
the records that its real publication or adoption used. Require the coordinator
to compare those observations and each agent to check the corresponding binding
against that epoch in process. Together these checks identify the source epoch
created in this run; matching requested labels are insufficient.

Resolve `n1` privately from the immutable original base admitted under J2.3:
read its note rows and choose the lowest original note ID. Retain its complete
baseline payload, note-type identity and card/deck relations as one unique
original-role witness. No manifest order or ID list is assumed. Both devices
derive that witness independently from their verified original copy of the same
generation. After adoption, the peer maps that original role to the actual
adopted note and verifies its identity and preserved content/relations below.
Never choose the lowest current row,
accept a caller's native ID or reuse smoke's added note. Missing, ambiguous or
inconsistent original-role evidence fails. Execute `edit` once through the normal
note updater. Generate `M-sync-now-n1-Q` inside the agent, using the admitted
marker's value for `M`, and write it to `Q`. Require different content from the
original base. Keep GUID, type, tags, every other field's bytes, flags/data and
the existing card/deck relations. Follow `notes/note-records` N2/U3 and
`storage/ids-usn-mtime` S3/S5 for normalization, sort text, checksum and stamps.
Check those computed values and stamps against their documented rules; baseline
row equality is not required for that metadata, and no override is permitted.
Flush the update and inspect its committed row. Retain the actual operation
provenance, revision, canonical digest and checks for both the changed field and
preserved content/relations. Export no field text or native identifier.

#### Original exchanges and peer evidence

Each `sync_now` invokes the existing coordinator manual exchange once, outside a
review session, and follows that exact issued exchange to its terminal
`RecordExchange`. No source mutation is required on the receiving iPhone. Neither
an old result nor smoke's four-entity ACK predicate substitutes for this result.
The Mac must report actual `succeeded` accounting and revision-specific durable
acknowledgement of the edited note. An earlier automatic acknowledgement may
satisfy that revision only with its original event/operation proof; do not
attribute it to the manual exchange. Report actual pending/paused/missing media
separately on both devices, without turning collection success into media success.
Do not issue another exchange to obtain a requested outcome.

The iPhone must perform its eligible fetch and normal durable intake/apply.
`observe` reads the adopted note through the actual current owned Host at a safe
barrier after that work. It returns the real canonical note digest, structural
relations and durable apply/row evidence under `n1`. This is a fixed read-only
inspection, not a script, query language or caller-provided expected observation.
The coordinator compares the complete canonical payload with the Mac's actual
committed payload, including the metadata that canonical projection represents;
do not mask metadata differences. It also requires the intended field change
and preserved content/relations above, with normal local metadata checks. Local source ACK, peer fetch
return, inbox presence or a peer memory cache cannot prove the peer holds the edit.
Missing durable observation fails; it does not trigger mutation or exchange replay.
`close` retains J3.1's coherent final integrity and joined-shutdown requirements.
After a relaunch, J5.1 states which process the host joins and which work that `close` joins.

#### Fresh runtime and interruption

Before any successor executes in a fresh app/runtime, validate the exact J2/J3
bindings and coordinator-retained journal prefix. Reopen the same run-owned
collection/tracking without installation, fresh publication or repeated adoption.
Re-establish account authority in J2 order and reconstruct only the mappings and
phase supported by actual durable operation/row/progress evidence. Completed
commands are not rerun, old sessions are not recreated, and journal phase alone
does not prove that a referenced entity or operation exists. Missing or changed
required progress fails before successor database/SDK work. This restoration is
internal to delivery/relaunch, not an extra command or an alternate launch position.

For intentional interruption in this slice, require `recovery_check` equal to
`sync_now.edit` on Mac position 4, and `sync_now.exchange` on either `sync_now`.
It is absent elsewhere; this slice grants no intentional interruption of bootstrap
or close. Apply J5 to any incomplete command. `sync_now.edit` must identify the
original committed operation and its once-only durable effect; expected payload
equality alone is insufficient. `sync_now.exchange` must identify that original
exchange's actual result and durable accounting, not run a new exchange. Both
require relaunch integrity and the same account/owner checks. An uncommitted
required edit, indeterminate exchange, missing recovery inspector or other
unproved interrupted outcome fails without replay or successor. Only proven
original completion permits `done` linked by `recovered_from`. A new runtime
after a completed command must also retain these no-repeat guarantees.

J4–J6 and Q5 apply throughout. Keep original callbacks and failure cancellation/
join ordering. Protocol/fake tests establish no signed execution. The selected
program needs both devices' real network and durable observations, cross-device
comparison and required ledger/check evidence before claiming the Q7.5 leg.
Unimplemented checks remain missing. This adds no other signed leg, full-lane,
soak, Q13 first-sync or owner-approval claim.

## J4. Durable content-free journal

`steps.jsonl` is an append-only UTF-8 JSON Lines file owned by the agent. Each complete line is one record. J5 permits repair of an incomplete final line only. The agent durably writes `started` before the command's first effect. It durably writes `done` or `failed` before reporting a terminal result. A durable append includes the storage flush needed for survival of an app kill. The directory entry must also survive the initial creation.

All records contain:

| Field | Meaning |
|---|---|
| `header_sha256` | Exact J2 header digest |
| `record_seq` | Consecutive journal position, starting at `1` |
| `command_seq`, `command_sha256` | Exact command this record describes |
| `agent_instance` | Run-local label for this process launch |
| `observed_at` | UTC instant of the observation |
| `kind` | `started`, `observation`, `done` or `failed` |

`started` records the action. It includes the observed account label only after the current identity check succeeds; startup must not claim an identity before that check. It asserts no successful operation. There is one `started` per command, including across relaunches.

`observation` contains facts from real operation receipts, engine events and checks. Event names are fixed; device, instance, operation and entity names use local labels. Allowed facts include counts, canonical payload digests, status values and CloudKit error codes. Write no CloudKit record, account identity or collection content. For errors, record the typed domain and code, never the error's free text.

Engine observations preserve callback order, the engine-instance label and the related operation label. Per-record result observations precede the state-save observation that reflects them. J4.1 states how the journal shows that order. The journal must retain enough actual order to judge Q7.6's A6; a summary saying A6 holds cannot replace those events. A state-save or apply observation references the actual durable operation it reports. Background callbacks may follow their command's `done`; they retain that command identity and never add another terminal result. Automatic callbacks without a causal command use the latest started command as the observation boundary, without claiming it caused them.

`done` contains the actual operation result and the checks required for that command by Q5 and the selected leg. Its `checks` entries name the check, its observed facts or evidence reference, and its outcome. It does not accept an expected result supplied by the command as an observation. Every completed local mutation links to the real core operation receipt or the durable entities and revisions that prove its effect. A Sync Now result comes from the real coordinator and its tracking, not a command's requested result.

`failed` contains a fixed failure reason, the observed check facts and every CloudKit error code encountered. It is terminal and stops further commands. A planned negative case may finish `done` when its expected error and required postconditions actually occur. An unexpected operation failure or failed check cannot be renamed a planned negative case afterward.

After failure, the agent stops accepting commands and cancels and joins the work it owns through the existing coordinator cancellation path. It retains late callbacks as `observation` records while that work ends. Those records cannot add a terminal result, change the failure or establish acceptance. No successor starts while owned work remains active. This adds no timer or repeated execution.

Run every check required by Q5 and the selected leg. The caller cannot select fewer checks. The agent records the app's checks when the contract requires them. The coordinator compares devices and service state using their actual observations. Checks that still need evidence stay open; the journal cannot report them as passed.

All command files, journals, copied logs and evidence pass Q5's `content-free` check. Use account labels only, never an address, user record id, credential or token. Core inspection can read generated content in process; it exports only the facts these checks need. This file does not define the final media aggregate encoding or replace Q7.4's final comparison.

### J4.1 Order evidence for engine observations

In J4, "precede" is the order in which one engine instance delivered its callbacks, as `sync/cloudkit-records` assumption A6 states it for one engine. Acceptance Q7.5's "event order in the step log" and Q7.6's A6 entry mean the same order. This section states only how the journal shows that order. It sets no ledger value: acceptance Q7.4 and Q7.6 judge A6 from the order shown here, unchanged.

A per-record result observation and the state-save observation that reflects it are identified only by their recorded references: the result's related operation label and the durable operation that the state save references (J4). Nearby records, instants and later durable state do not identify that state save.

The journal shows the order of such a pair only from records of one engine instance: the same `agent_instance` and the same engine-instance label.

- Append order (`record_seq`) is the pair's shown order, subject to the next rule.
- If both observations also carry a callback-order fact, such as a tick, under the same fact name, the pair has a shown order only when that fact gives the same strict order as append order. When it gives the other order, or equal values, the journal shows no order for the pair. Neither form overrides the other: the journal does not record when the agent assigned the fact or when it appended the record relative to the callback, so neither is taken as the delivery order.

An agent that appends callback observations out of their delivery order, for example after buffering them, therefore does not show the order J4 requires, whatever order fact it records.

The journal also shows no order:

- for a pair whose observations belong to different engine instances or `agent_instance` values, including an original process and the process relaunched after it (J5.1);
- for a result and a state save whose recorded references do not link them;
- for a result the journal does not hold, such as an observation that the original process buffered and a kill lost (J5.1). The journal does not show that every result reached it; a check that needs a result the journal does not hold has missing evidence (J5).

`observed_at` instants, summary facts, durable rows and later state supply no missing order or link. When J4, a leg check or a J5 recovery check requires the order of a pair and the journal shows none, that is a failed evidence check (acceptance Q7.4), and the checks that depend on it fail. The failure names the pair and the order each form gives.

A callback-order fact orders observations only. It does not move an observation to another command, `agent_instance`, engine instance or terminal state.

## J5. Relaunch and interrupted commands

The coordinator kills and relaunches the actual app as Q7.3 specifies. The agent cannot survive in another process. On relaunch it validates J2's runtime bindings and replays the journal as evidence, without replaying completed operations.

The complete journal prefix must match the command prefix and its state transitions. A terminal record requires its earlier `started`; a command has at most one terminal record. Observations must belong to its started command. A new command cannot start after an incomplete or failed predecessor. Repeated record numbers, gaps, mismatched digests and new commands or terminal records after failure invalidate the journal. J4 permits only late observations while failed work ends. The journal must also extend the coordinator's retained prefix as J3 requires.

An incomplete final journal line supplies no evidence. Before continuing, preserve its bytes as diagnostic evidence and durably remove only that incomplete suffix. Retain every complete line unchanged, then apply the recovery rules below. A malformed complete line, or an incomplete line anywhere else, invalidates the journal. No reader silently skips a damaged complete record.

When a command has `started` but no terminal record, the agent first opens the same core and tracking state. It executes the leg's `recovery_check` against the actual durable state and, where required, real CloudKit observations. It records those observations with the original command identity and the new `agent_instance`.

The check must distinguish the committed operation from one that never committed or has an indeterminate outcome. It verifies the leg's expected recovery, including required integrity checks after relaunch. A kill does not excuse a lost committed write (Q5). Neither a copied `done` line nor the presence of an expected digest alone proves that this app executed the operation.

Only a successful recovery check permits a `done` record with `recovered_from` naming the original `started` record. It proves the planned interrupted outcome; it does not reissue a mutation to obtain that outcome. Missing recovery instructions, missing evidence, a failed check or an indeterminate outcome stops the run. Failed or indeterminate recovery records `failed`; the agent cannot continue to the next command.

If the journal ends after a terminal `done`, the next command is its successor. If it ends before any `started` for the next accepted command, that command may start normally. These are progress rules for this agent, not a new sync retry mechanism. Recovery and normal execution retain the product's existing account fences, deadlines and cancellation rules.

### J5.1 Process boundary at a relaunch

This section applies when the host coordinator relaunches the app after a kill, or after it terminated the app for an offline period (acceptance Q7.3, J5). It grants no interruption or recovery that J3.2 and J5 do not grant.

The original process ends with all in-process work it owned: the work of its in-app sync coordinator (`SyncCoordinator`), its engine instance and that instance's callbacks. No later process can cancel, drain or join that work. A request that the original process had sent may still take effect on the CloudKit service; J5's recovery check decides that outcome, which may be indeterminate.

What the original process leaves is evidence, not work. That evidence includes the durable collection, tracking and media state; the device-local run directory (J2, J3); the complete journal records the original process appended, each with its original `agent_instance`, command identity and `record_seq`, and for engine observations its engine-instance and operation labels (J4); and the host coordinator's retained copies of the delivered commands and the journal prefix (J3). Observations the original process never appended are lost. They are not evidence, and nothing stands in for them (J4.1). The preserved bytes of an incomplete final line are diagnostic evidence only (J5). This section does not change how service state is shown: by journaled observations, by the peer's observations and by the host coordinator's comparison (J3.2, J4).

This evidence is input to the J5 recovery check of the interrupted command and to J3.2's reconstruction of mappings and phase, under their rules; journal phase alone does not prove that a referenced entity or operation exists (J3.2). The relaunched process makes every check of a recovered `done` itself, including Q5 `results`, recomputed as Q5 states. An original `done` line or check outcome is input to that check, never its outcome. Recovery observations, including any real CloudKit observation J5 requires, carry the original command identity and the new `agent_instance` (J5).

Original records stay where they are, with their original `agent_instance`, engine-instance label and command identity. The relaunched process does not relabel them as its own engine instance's work or as a later command's work, and it does not attribute an automatic callback to the manual exchange. An original acknowledgement whose manual or automatic cause is unresolved is never reported as the manual exchange's own acknowledgement. It may satisfy the revision only through J3.2's earlier-automatic acknowledgement rule, with its original event/operation proof. An acknowledgement event that the relaunched process's engine delivers is neither the original exchange's acknowledgement nor an earlier one, so it cannot satisfy the interrupted command's revision-specific acknowledgement. Durable acknowledgement state counts for the interrupted command only together with the original exchange's actual result (J3.2) or, for an earlier automatic acknowledgement, its original event/operation proof; a durable row alone does not show which process wrote it.

The relaunched process owns only the work it starts: its in-app sync coordinator's work, its engine instance and that instance's callbacks. A `close` in that process applies J3.1 to that work, in J3.1's order: actual cancellation/drain, join of the owned work, the final `close.integrity` snapshot, Core close, then `close.joined`. `close.joined` reports only that process's own work, with every fact J3.1's table requires of it. It never claims that the original process, its engine or its callbacks were drained, joined or closed. Where J3.2 applies J3.1's close rule after a relaunch, the app-hosted process that the host joins is the one that ran `close`; the host never joins the killed process. The kill and the original process's exit are not a clean close. A recovered `done` permits its successor under J5; it supplies neither `close.integrity` nor `close.joined`. The host coordinator still checks each terminal result and its evidence, and compares the devices' observations, before it delivers `close` (J3, J3.2, J6). Each further relaunch starts a new process boundary under the same rules.

This adds no deadline, retry, account permission or cancellation rule. J5's account fences, deadlines and cancellation rules apply unchanged.

## J6. Coordinator evidence and result scope

The coordinator validates header and command bytes, journal transitions, actual build bindings and the evidence behind operation outcomes. It retains the exact delivered commands and returned journal as MG artifacts. It rejects duplicate JSON keys, invalid UTF-8, non-finite numbers and unsupported protocol versions. JSON shape validation alone cannot validate a device operation.

A protocol self-test may exercise parsing, duplicate delivery and interruption with synthetic state. It cannot produce `cloud-signed` execution evidence. A real signed smoke must exercise a generated-data operation through the app, real adapter and coordinator, and observe its actual result. A smoke records only that coverage. It fills no Q7.5 leg or assumption merely because it uses this protocol.

A selected leg needs all of that leg's required devices, steps, checks, actual network execution and ledger observations. A protocol producer that implements only smoke must refuse other selections without claiming they ran. A passing selected leg does not set `details.scenarios.cloudkit-signed` to pass. Q7.4's whole-lane validator and Q7.6's full applicable ledger remain required. This protocol, its tests and build receipts cannot satisfy `u248-soak`.

Keep the existing MG envelope. If setup or an assisted user step is missing, record `user_steps`, `status: warn` and `details.outcome: awaiting_user`. For machine blocks, use MG's queued result. Both remain incomplete. Missing required non-assisted work, false assumptions and failed evidence checks fail under Q7.4. The protocol grants no user-step approval.

If required code is missing, fail with `implementation_missing`. This includes the generator, preparation code, receipt checks, account input checks, agent and required proof of the real operation. Missing code is never a user step. Use `awaiting_user` only for missing user-owned setup or the approved expected-account choice under Q7.2.

Q7.3 allows pause or an app stopped by the coordinator for one device's offline period. Legs that require network loss still need the VM's network cut. Existing rules for the host screen, VM and account setup continue to apply. Preserve Q9.2's three devices, eight hours of work, kills, network cuts and quiescence limit.

## J7. Executable acceptance cases

Protocol tests exercise the following cases without credentials. They check operation counts as well as parser results. Their outputs are protocol-test evidence only.

| Case | Required observation |
|---|---|
| Deliver one valid command twice | One actual operation, one `started`, one terminal result; second delivery adds no effect |
| Change, remove or reorder an accepted command; deliver a sequence gap | Refusal before another operation |
| Substitute run, source, build, device, container, environment or account scope | Binding refusal; wrong container/environment/account uses `wrong-container`; zero CloudKit database operations; product/container rejection also permits no account lookup |
| Copy another device's valid journal or a `done` without its `started` | Refusal; copied outcomes establish no execution |
| Kill after durable `started` and before local commit | Check the leg's expected recovery on relaunch; do not repeat the mutation or start the next command without that evidence |
| Kill after local commit and before `done` | Relaunch observes the original operation exactly once, runs the recovery check, then records linked recovery completion |
| Recovery fails or cannot determine the outcome | Terminal failure; successor never starts |
| Deliver a successor after `failed` | No successor effect and no automatic repeated execution |
| Truncate a final record, damage a complete record or repeat a terminal record | Incomplete tail gives no success; damaged complete journal or duplicate terminal refuses continuation |
| Inject content markers into commands, logs or diagnostic errors | `content-free` fails; raw content never appears in a passing artifact |
| Supply only expected outcomes, build receipts or synthetic engine callbacks | No signed pass or signed ledger observation |
| Fail product/container checks, or fail the later account match | No lookup before product/container checks; only read-only identity lookup before account match; no database operation or cleanup |
| Receive startup callbacks or fail with work still active | Durable `launch` start precedes core and engine startup; failure cancels and joins owned work; late observations cannot change the result or start a successor |
| Return a journal shorter than, or different from, a prefix the coordinator retained | Refusal before further core or sync work, including after relaunch |
| Change executable bytes, build/deployment references or checked product grants | Refusal; an executable digest alone establishes no signed product |
| Give account input a wrong owner/mode, link, changed bytes or wrong header/run/device/scope | Refusal before database work; no private identity or input digest appears in exported diagnostics |
| Omit the user's approved expected accounts, or use a current account read to choose them | Await user setup when its expected accounts are missing; refuse to create expected accounts from a current read |
| Change a fixture receipt, manifest, frozen map, file, prepared-source binding or destination owner | Refusal before installation or core startup; generated markers supply no proof |
| Relaunch with an existing run-owned collection, or direct initial install to the default collection | Preserve run data on relaunch; refuse default-collection delivery; no repeated install |
| Omit a required generator, input inspector, receipt verifier or agent implementation | Failure `implementation_missing`, never a pass or a user setup step |

App-hosted tests verify the producer separately: real core mutations and operation receipts; real coordinator results; actual adapter event order; durable flush ordering; interruption recovery; and rejection before cloud work under a wrong runtime binding. Fake engine cases stay fake evidence.

A signed smoke uses those components with real CloudKit, generated data and J2's verified bindings. Keep evidence of the actual operation and its checked outcome. Missing setup remains `awaiting_user`. Check that Release contains neither the agent nor cleanup hooks. Q7 still requires the selected-leg runs and the complete signed lane against real CloudKit.
