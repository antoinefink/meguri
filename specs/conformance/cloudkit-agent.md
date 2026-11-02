# Signed CloudKit device agent

Spec: conformance/cloudkit-agent
Requirements: R2047, R2048, R2049, R2050, R2051, R2052, R2053, R2054, R2055, R2056, R2057, R2058, R2059, R2064, R2065; supports R2066
Anki: none (Meguri design)
Scenarios: none (protocol tests, app-hosted `CloudKitSoakTests`, and signed runs of conformance/cloudkit-acceptance Q7)

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

### J2.1 Executable and receipt binding

`executable_sha256` hashes only the executable file, never an undefined whole-bundle encoding. It has the meaning of the existing MG `standalone_product.executable_sha256` field. The producer must provide this check for Mac, iPhone and iPad products; a Mac preview result cannot stand in for another device's product.

`build_receipt` and `deployment_receipt` each contain `{job, run_id}`. Resolve the exact MG receipts and only their declared artifacts using Q13.1's path and envelope rules. Require the run's clean source commit and the actual device/product binding. The coordinator independently checks the product signature, required CloudKit and push grants, provisioning under the existing platform rules, Debug/test-hook configuration, development container and environment, and the product actually delivered to this device. The running executable's bytes must match `executable_sha256` and that checked delivery. An executable hash alone proves none of the signing or account conditions. A caller-written receipt or a standalone preview's product details cannot substitute for these checks.

### J2.2 Protected expected accounts

The host reads `Meguri/CloudKitSignedPrivate/<run_id>/<device>/expected-accounts.json` under the current user's Application Support directory. The device reads the same relative path under its app container's Application Support directory. Keep both copies outside the checkout, `build/`, fixture directories and public MG artifacts. The coordinator copies only the account choice supplied by approved Q7.2 setup, with this run and device binding. Reading the current account cannot create or change the expected account.

The private directories have mode `0700`. The input is a regular file with mode `0600`, one link, and the process user's ownership. Check that owner on the private directories too. Read through stable no-follow file and directory bindings; reject links, paths writable by another user, changed file bytes and changed path authority. These checks start at the admitted Application Support root, following the existing protected-input pattern. The host and device copies must preserve these restrictions.

The JSON object contains exactly `protocol_version`, `header_sha256`, `job`, `run_id`, `source_commit`, `device`, `executable_sha256`, `container`, `environment`, `account_scope` and `accounts`. The version is `1`; every binding field equals J2's header. `accounts` maps exactly the header's account labels to the actual expected CloudKit user-record identities supplied by completed Q7.2 setup. Each value is a nonempty array of integers from 0 through 255: the exact opaque identity bytes the core uses under `sync/cloudkit-recovery#V9.1`, with no case conversion or hashing. Parse strictly, including duplicate-key rejection. Compare the actual core-verified current identity with this private input before database work, using J2's startup order.

Examples, commands, journals and public artifacts contain no account IDs. Keep the private input and its digest out of exports. Do not export an account hash or another value that identifies the same account between runs. Report failures with fixed labels and check results only. If the user has not supplied the approved expected accounts, await Q7.2 setup. A current identity that differs fails `wrong-container`. Missing code to read, deliver or compare the input fails under J6; it is not a user setup step.

### J2.3 Initial generated data

`fixture_delivery` contains `base`, `fixtures_receipt`, `source`, `manifest`, `frozen_input` and, only when preparation is needed, `preparation_receipt`. Receipt references are `{job, run_id}` under J2.1's MG resolution rules. `fixtures_receipt` identifies the real passing MG `fixtures` run. `source` is its declared generated fixture artifact. `manifest` and `frozen_input` each contain `path`, `sha256` and `bytes`. The manifest path is relative to `source`. The frozen-input path is exactly `fixture-input.json` in this device's run directory, outside the fixture source. Resolve neither through links or a parent-path escape. They identify exact input files, not a later export from the app.

`base` names the Q4.2 base the selected work requires. The initial producer may implement only `small`; it must not claim that other bases or a complete leg ran. `small` is not a currently defined literal MG fixture-job argument. The producer needs a real generated request and, if necessary, an actual MG preparation run that establishes Q4.2's base. Missing that producer or its checks is implementation work, never proof supplied by a `generated` marker.

The frozen JSON input contains exactly `protocol_version`, `run_id`, `device`, `source_commit`, `base`, `parent_files`, `prepared_source` and `files`. The version is `1`; run, device, source and base match the header. `parent_files` maps every source-relative parent file path to `{sha256, bytes}`. `prepared_source` is either that same source or the prepared fixture artifact declared by `preparation_receipt`. `files` is the corresponding exact source-relative file map for the data to install. Both maps include the database, manifest, any required WAL and every media file. Check exact file sets and actual bytes, not only declared digests. The real generator request, manifest and any preparation evidence must establish the generated base and its recorded changes. Preserve the parent's manifest; do not claim its digest describes prepared database changes.

Freeze those inputs before launch. The header binds the frozen input's bytes; its run/device/source fields bind it back without a circular header hash. The full maps and their generated media names are fixture input, never text to copy into logs or signed evidence. Public observations use fixed file labels, sizes and digests only. A returned collection or later app state cannot become the expected source.

Deliver a copy to a directory owned by this run and device, using the existing frozen-input and `FixtureInstaller` patterns. Check owner, non-writable-by-other paths, stable no-follow regular files and the complete frozen file set before installation. The destination is a new collection directory owned by this run inside the development app container; it is never the app's default collection or the daily-driver collection. Require its absence before the initial install, and check copied bytes before opening `CoreHost`.

Run the installer once, during the device's first `launch`, after durable `started` and before opening the local core. On relaunch, open the same collection and tracking owned by this run; do not install again. After a kill during that first install, check the actual state against the planned recovery. Do not delete the destination or copy the source again to obtain a passing result. The fixture's identity does not prove a study operation. Retain J4's actual operation evidence and all Q5 checks.

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

Engine observations preserve callback order, the engine-instance label and the related operation label. Per-record result observations precede the state-save observation that reflects them. The journal must retain enough actual order to judge Q7.6's A6; a summary saying A6 holds cannot replace those events. A state-save or apply observation references the actual durable operation it reports. Background callbacks may follow their command's `done`; they retain that command identity and never add another terminal result. Automatic callbacks without a causal command use the latest started command as the observation boundary, without claiming it caused them.

`done` contains the actual operation result and the checks required for that command by Q5 and the selected leg. Its `checks` entries name the check, its observed facts or evidence reference, and its outcome. It does not accept an expected result supplied by the command as an observation. Every completed local mutation links to the real core operation receipt or the durable entities and revisions that prove its effect. A Sync Now result comes from the real coordinator and its tracking, not a command's requested result.

`failed` contains a fixed failure reason, the observed check facts and every CloudKit error code encountered. It is terminal and stops further commands. A planned negative case may finish `done` when its expected error and required postconditions actually occur. An unexpected operation failure or failed check cannot be renamed a planned negative case afterward.

After failure, the agent stops accepting commands and cancels and joins the work it owns through the existing coordinator cancellation path. It retains late callbacks as `observation` records while that work ends. Those records cannot add a terminal result, change the failure or establish acceptance. No successor starts while owned work remains active. This adds no timer or repeated execution.

Run every check required by Q5 and the selected leg. The caller cannot select fewer checks. The agent records the app's checks when the contract requires them. The coordinator compares devices and service state using their actual observations. Checks that still need evidence stay open; the journal cannot report them as passed.

All command files, journals, copied logs and evidence pass Q5's `content-free` check. Use account labels only, never an address, user record id, credential or token. Core inspection can read generated content in process; it exports only the facts these checks need. This file does not define the final media aggregate encoding or replace Q7.4's final comparison.

## J5. Relaunch and interrupted commands

The coordinator kills and relaunches the actual app as Q7.3 specifies. The agent cannot survive in another process. On relaunch it validates J2's runtime bindings and replays the journal as evidence, without replaying completed operations.

The complete journal prefix must match the command prefix and its state transitions. A terminal record requires its earlier `started`; a command has at most one terminal record. Observations must belong to its started command. A new command cannot start after an incomplete or failed predecessor. Repeated record numbers, gaps, mismatched digests and new commands or terminal records after failure invalidate the journal. J4 permits only late observations while failed work ends. The journal must also extend the coordinator's retained prefix as J3 requires.

An incomplete final journal line supplies no evidence. Before continuing, preserve its bytes as diagnostic evidence and durably remove only that incomplete suffix. Retain every complete line unchanged, then apply the recovery rules below. A malformed complete line, or an incomplete line anywhere else, invalidates the journal. No reader silently skips a damaged complete record.

When a command has `started` but no terminal record, the agent first opens the same core and tracking state. It executes the leg's `recovery_check` against the actual durable state and, where required, real CloudKit observations. It records those observations with the original command identity and the new `agent_instance`.

The check must distinguish the committed operation from one that never committed or has an indeterminate outcome. It verifies the leg's expected recovery, including required integrity checks after relaunch. A kill does not excuse a lost committed write (Q5). Neither a copied `done` line nor the presence of an expected digest alone proves that this app executed the operation.

Only a successful recovery check permits a `done` record with `recovered_from` naming the original `started` record. It proves the planned interrupted outcome; it does not reissue a mutation to obtain that outcome. Missing recovery instructions, missing evidence, a failed check or an indeterminate outcome stops the run. Failed or indeterminate recovery records `failed`; the agent cannot continue to the next command.

If the journal ends after a terminal `done`, the next command is its successor. If it ends before any `started` for the next accepted command, that command may start normally. These are progress rules for this agent, not a new sync retry mechanism. Recovery and normal execution retain the product's existing account fences, deadlines and cancellation rules.

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
