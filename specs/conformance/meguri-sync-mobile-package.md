# Offline Mobile10 package companion

Spec: conformance/meguri-sync-mobile-package
Requirements: Meguri Sync server plan R6, R7–R10, R14, R32; F3; AE4
Anki: original Meguri preparation policy; current Desktop SDK 26.09.4 qualification pending
Scenarios: conformance/recordings/scenarios/meguri-sync-server/

This contract defines two offline preparation operations. It changes no selected Mobile10 case, action, assertion, recipe or count and supplies no execution credit.

## M1. Two offline operations

Run the pinned Desktop SDK in an MG-owned qualified environment without network, credentials, personal profiles or a server connection. Exactly two helper selectors exist: `export.colpkg` exports the assigned closed synthetic Desktop B collection; `import.colpkg` imports one explicitly staged Mobile whole-collection package into a fresh empty owned SDK destination. These are not new outer case operations. Neither operation synchronizes, reviews, edits, repairs, enables scheduling policy, activates a live collection or grants Mobile credit.

Use latest whole-collection `.colpkg`, media included, older-client mode off: E9's `media=true`, `legacy=false`. The current SDK surface may name the media option `include_media`; its exact public call and effect must be qualified at 26.09.4. There is no whole-collection scheduling option. Preserve all collection history/scheduling and every media file, referenced or unused. Refuse legacy/deck/subset packages at this narrow boundary; product support for other formats is unchanged. Current SDK support/effects are a later focused-test prerequisite, not a fact inferred from 26.09.3.

## M2. Exact pre-execution identity and control input

All JSON inputs are UTF-8. New boundary validators must reject duplicate members, unknown members, wrong types and unresolved artifact references at every defined object level. This is a new admission requirement; it is not a claim about existing snapshot consumers. SHA-256 strings are 64 lowercase hexadecimal characters; byte lengths are nonnegative integers, never booleans. Artifact descriptors below have exactly `role`, `sha256`, `bytes` and use existing schema roles.

`/in/invocation.json` is one proposed fixed offline variant, `meguri-mobile-package-invocation-v1`. It retains the public reference invocation's source/run/session fields and adds the marker and actual client/image identities below. Its exact keys are:

| Key | Required value |
| --- | --- |
| `format` | `meguri-mobile-package-invocation-v1` |
| `source_commit` | Actual clean qualified tooling source commit, 40 or 64 lowercase hex |
| `batch_base` | `480df4526514e294f5a3a16dd4ab83a1fbcc7f47` |
| `source_sha256` | Actual qualified tooling input digest, using the current reference-image input-digest rule |
| `lock_sha256` | SHA-256 of the actual pinned tooling requirements lock bytes |
| `mg_job` | `sync-server-reference` |
| `arguments` | Exactly `["mobile-package","export.colpkg"]` or `["mobile-package","import.colpkg"]` |
| `run_id` | Actual owning MG run ID, nonempty string, compared with trusted MG state |
| `author_session` | Actual responsible qualified author session, nonempty string |
| `source_author_sessions` | Nonempty unique array of responsible qualified session IDs, resolved against committed public session records |
| `client` | Existing closed schema client object: `kind=desktop`, `version=26.09.4`, exact `build` and `runtime_sha256`; no other keys |
| `tooling_image_id` | Actual immutable `sha256:` plus 64-lowercase-hex image ID |

The fixed `mobile-package` mode is proposed, not claimed to exist. Trusted MG builds/checks the current tooling image, measures its identity and pinned runtime, verifies source/digest/lock/session identity, then writes this invocation **before** launching the helper. It compares the supplied invocation byte-for-byte with that trusted copy, checks the actual launched image/arguments/no-network/mounts, and retains the real MG invocation/result separately. No future MG result, server after-inspection or retained-peer invocation is required or fabricated. This offline source identity is tooling identity; it does not replace the formal candidate server/image source binding.

`/in/action.json` is the only control input. Its exact common keys are:

| Key | Required value |
| --- | --- |
| `format` | `meguri-mobile-package-action-v1` |
| `operation` | Exactly one of M1's selectors, agreeing with invocation arguments |
| `invocation_sha256` | Exact `/in/invocation.json` bytes, also retained in input CAS |
| `selection_sha256` | Frozen full `candidate-mobile-final-selection.json` bytes |
| `recipe_sha256` | Exact recipe bytes, agreeing with that selection's `fixture_recipes_sha256` |
| `review_sha256` | Accepted independent preparation review covering rules/code/applicable initial inputs |
| `case` | One exact case ID in that full selection |
| `action` | Zero-based integer index within that case's `actions` |
| `actor` | Exactly that action's actor: `client_b` for export, `client_a` for import |
| `before_sha256` | Frozen authoritative baseline snapshot for that action |
| `input` | One M3 input artifact descriptor |

Import additionally requires exactly `staging_sha256` under M3; export forbids it. Resolve the selected object from the full frozen selection, exact case ID and action index. There is no separate action hash or JSON reserialization rule. The converter remains Desktop even when `actor` identifies a Mobile-origin action. The selection remains ten cases/22 actions: SHA-256 `7ca1cff89f051fc845c0e2bc31d140306f67432be5de8c5c31fe4bc70aa2b87d`; recipe SHA-256 `f8f8488500dc11c8d3ce0da7185d0de280160b240da38a0cba11b94457902348`.

The finite input artifact closure comprises the invocation, full selection, recipe, preparation review, baseline snapshot, `input`, import staging record/evidence when applicable, and export inventory objects when applicable. Only the named artifact slots in this contract and the independently admitted finite evidence resolver in M3 expand that closure; do not guess references from arbitrary JSON keys, text or filenames. Every artifact digest resolves only `/in/artifacts/<sha256>`; no artifact supplies a filesystem selector or external URL. Tooling source/input digest and runtime/image identities are compared with trusted MG's measured build/pin data; they are identities, not instructions to find arbitrary files. Retain that actual build/pin/lock/session proof with the MG run before admission. A self-consistent supplied digest set alone is insufficient.

## M3. Inputs, scope provenance and fixed paths

For export, `input.role=synthetic_input`. Its bytes are a closed collection inventory with exactly `database` and `media`: `database` is a `neutral_observation` artifact descriptor for the self-contained closed B collection; `media` is the whole exact filename map to `{sha256,bytes}`. All referenced bytes are in CAS. Trusted MG captures this only from its assigned named B collection, checks the full typed snapshot/media against `before_sha256`, and the preparation review binds its recipe/authority. No caller chooses a source profile/name/path. This two-member inventory is an explicit proposed portable input shape, not a receipt or a full SDK profile export.

For import, `input.role=package` identifies exactly `/in/mobile.colpkg`. `staging_sha256` resolves one closed original-policy staging record with exactly:

| Key | Required value |
| --- | --- |
| `format` | `meguri-mobile-package-stage-v1` |
| `invocation_sha256` | Same offline invocation as the action |
| `run_id`, `case`, `action`, `actor`, `before_sha256` | Exact agreement with trusted invocation/action/baseline; actor `client_a` |
| `package` | Same package descriptor as `input` |
| `export_operation` | Exactly `whole_collection` |
| `device_evidence` | Existing-role `device_evidence` artifact descriptor for the actual installed Mobile26.10 build/OS/device and this execution |
| `export_evidence` | Existing-role `device_evidence` artifact descriptor for the actual supported whole-collection export and its staged output-byte handoff |
| `offline_evidence` | Existing-role `neutral_observation` descriptor for trusted MG's actual blocked-submission/capture-order record |

Trusted MG resolves these descriptors from CAS and requires the actual device/export evidence to agree on the same device/build/OS, run, case/action, baseline and package digest/length. `export_evidence` must establish that the observed operation exported the **whole collection**, with media enabled and older-client mode disabled, and that the explicitly handed-off file is these exact bytes. A stage-record assertion, screenshot alone, `.colpkg` name or valid container cannot establish that fact: deck and collection packages share a container. Refuse renamed deck/subset exports and missing scope/handoff proof; invent no content heuristic. Original evidence may be one shared artifact if it independently supplies both device and export bindings.

For every Mobile-origin event, the immutable prelaunch `offline_evidence` must establish that candidate submission was blocked before that event/export and that the actual export/package freeze already occurred. It identifies this same run/case/action/package/baseline and does not attest to or hash future conversion outputs. After helper completion, trusted enclosing MG separately records and binds the actual conversion result and complete closed-CAS freeze, before any unblock or first later candidate submission. Final admission verifies both observed phases and their shared identities/order; no input record can substitute for the later freeze evidence. For a post-receive witness, the prelaunch evidence binds that explicit checkpoint/export/package freeze, and MG binds the completed conversion freeze before subsequent actions; neither phase retroactively proves an earlier state. Use the enclosing MG's actual ordered records, not fixture timestamps, with no added fields or observer pipeline. Existing C11 device evidence has no public inner grammar establishing all these facts; **before a staged Mobile package is admitted**, the finite producer must supply its actual evidence payload and resolver for independent review against these exact checks. This is a named remaining evidence-admission prerequisite, not an invented device receipt or a gate on authoring the offline converter.

Only `/in`, `/out`, `/state` are mounted. `/in` is read-only: `action.json`, `invocation.json`, `artifacts/<sha256>`, plus exactly one `mobile.colpkg` for import. Export reconstructs its verified working copy only at `/state/client_b`; import creates previously absent `/state/receiver`. Scratch is fresh and owned; `/out` starts empty. No paths, profiles, URLs, commands, export options or budget overrides come from callers. No native profile/Downloads scanning occurs. All staged/CAS inputs are regular single-link files; reject symlinks, special/multiply linked files, extra package inputs, missing/duplicate/swapped/changed/stale/partial artifacts. Write no original input; remove only operation-owned scratch.

## M4. Bounded inspection and retained result

Apply `package/container#P2`–`P6`, `P8`, `P9`, latest `P16.1` completeness as this boundary's defensive policy. Fixed P8 limits: 400,000 central entries; metadata 64 KiB; examined scheduler marker 4 MiB; decoded media map 64 MiB; decoded collection 8 GiB; collection/map expansion at most `max(16 MiB,1000×compressed-entry bytes)`; each media payload at most its declared decoded size; decoded content total 16 GiB; zstd window at most 2^27 bytes. Checked arithmetic and streaming stop at the first breach. These are existing Meguri limits newly required here, not SDK guarantees. Qualify enforcement at this boundary; do not build a new general package framework or activate Meguri's live replacement/backup path.

Validate collection, complete media map and every mapped payload, exact names, lengths, CRC and SHA-1 before success. Refuse duplicate central names, unsafe/colliding media names, missing payloads or unclaimed numeric payloads under latest rules. Unknown nonnumeric entries keep P2's handling and are never filesystem paths. Fresh destination media starts empty; no stale files or SDK reuse/merge shortcut may satisfy completeness.

Publish `/out/observation.json` only after completion/closure/rehashing, as one existing-schema observation exactly `{name,value}` with `name=mobile_package_preparation`. Its closed `value` has exactly:

| Key | Required value |
| --- | --- |
| `input_manifest_sha256` | Hash of complete `/in/action.json`, binding all input identities transitively |
| `client` | Exact Desktop client object from the trusted invocation |
| `package` | Original actual package descriptor, role `package` |
| `package_database` | Original decoded collection-member bytes before SDK open/import, descriptor role `neutral_observation` |
| `package_snapshot` | Complete typed raw twelve-table snapshot of those original bytes, same role |
| `database` | Closed B working-copy database (export) or closed fresh SDK destination (import), same role |
| `snapshot` | Complete typed raw twelve-table snapshot/whole media map of that closed state, same role |
| `media` | Whole exact filename map to `{sha256,bytes}` for every decoded media file |
| `case_credit` | Exactly `false` |

Retain every descriptor's actual bytes, every media payload and the full input closure under `/out/artifacts/<sha256>`. Original package/database bytes are unchanged, never hashes alone. Closed databases are self-contained, with no required rows only in an unretained WAL/journal. Equal content may share a CAS object; equal bytes under two names retain both map entries. Keep the original staged package even on import failure. Retain partial failure artifacts separately in the MG run; never publish/admit them as a successful observation. Recheck all referenced bytes/lengths and both raw snapshot/media equalities before admission.

## M5. Complete existing raw snapshot format

Use `meguri-retained-snapshot-v1`, with exactly `format`, `tables`, `media_sha256`; format value is that literal. `tables` has exactly the twelve tables below. Each table has exactly `columns`, `rows`: the exact ordered column array below, and complete row arrays of the same width. Empty tables have `rows:[]`; do not fabricate missing rows/tables or replace original package state with upgraded state. Refuse unsupported pre-open schemas. JSON object-member ordering is not semantic; array ordering is.

| Table | Exact ordered columns |
| --- | --- |
| `col` | `id crt mod scm ver dty usn ls conf models decks dconf tags` |
| `notes` | `id guid mid mod usn tags flds sfld csum flags data` |
| `cards` | `id nid did ord mod usn type queue due ivl factor reps lapses left odue odid flags data` |
| `revlog` | `id cid usn ease ivl lastIvl factor time type` |
| `graves` | `oid type usn` |
| `deck_config` | `id name mtime_secs usn config` |
| `config` | `KEY usn mtime_secs val` |
| `tags` | `tag usn collapsed config` |
| `fields` | `ntid ord name config` |
| `templates` | `ntid ord name mtime_secs usn config` |
| `notetypes` | `id name mtime_secs usn config` |
| `decks` | `id name mtime_secs usn common kind` |

Each cell is an object with exactly one member: `null:true`; `integer` containing a canonical signed decimal string (the entire string matches `0` or `-?[1-9][0-9]*`, within −9223372036854775808 through 9223372036854775807); `real_bits` containing exactly 16 lowercase hex digits of the binary64 IEEE bit-pattern integer, high nibble first; `text_hex` or `blob_hex` containing zero or more lowercase byte pairs preserving actual stored bytes and SQLite storage class. Examples: `{"integer":"-47"}`, `{"real_bits":"400c000000000000"}` (3.5), `{"real_bits":"8000000000000000"}` (negative zero), `{"text_hex":"4c756d656e"}`, `{"blob_hex":"0207f1"}`, `{"null":true}`. No untagged numbers, `protocol_stamp` raw cells, Unicode re-encoding, protobuf rewriting, ID remapping or unknown-byte stripping. Do not infer finite-only REAL canonicalization; retain actual emitted bits.

Rows are strictly ascending by these keys: signed `id` for `col`, `notes`, `cards`, `revlog`, `deck_config`, `notetypes`, `decks`; signed `(oid,type)` for `graves`; signed `(ntid,ord)` for `fields` and `templates`; raw TEXT `KEY` under SQLite BINARY for `config`; TEXT `tag` under the registered pinned unicase 2.6.0 comparator (`text/unicode-primitives#U2`) for `tags`. Composite keys compare first component then second; numeric order is signed numeric, not decimal-string order. Key cells must have INTEGER/TEXT classes as specified. The row widths, respectively, are those exact column-array lengths above. Reject duplicate keys under their actual comparator, including unicase-equal tag keys; do not add an invented tie-break or repair their order.

Config BINARY order compares actual stored bytes under the collection's UTF8 text encoding. For tag-key comparison only, invalid UTF8 bytes are interpreted with lossy UTF8 replacement and then the same pinned unicase comparator; raw `text_hex` retains the original bytes. No locale/normalization/platform folding or secondary BINARY/code-point tag order is permitted. If that exact comparator/key domain cannot be supported, refuse it explicitly rather than change bytes or silently substitute ordering. Non-key TEXT remains arbitrary stored bytes, not a requirement for valid Unicode.

The existing table traversal sequence is `col, notes, cards, revlog, graves, deck_config, config, tags, fields, templates, notetypes, decks`; JSON `tables` member order is not semantic. A complete paged read has no starting sentinel, includes minimum signed 64-bit/empty text keys, applies no pending-USN filter, and continues strictly beyond the preceding last **complete** key using the same comparison. Page boundaries are not serialized: the existing interface returns at most 250 rows/nominal 8 MiB per page, allowing one larger row alone; this is not a total limit or permission to truncate. Capture only stable owned closed database bytes; the public pager alone does not establish a whole-capture transaction against concurrent mutation.

This complete structural example includes all table/column objects and empty row arrays; it illustrates serialization, not a valid populated collection or an admitted fixture. The typed-cell examples above show every cell alternative.

```json
{"format":"meguri-retained-snapshot-v1","tables":{
"col":{"columns":["id","crt","mod","scm","ver","dty","usn","ls","conf","models","decks","dconf","tags"],"rows":[]},
"notes":{"columns":["id","guid","mid","mod","usn","tags","flds","sfld","csum","flags","data"],"rows":[]},
"cards":{"columns":["id","nid","did","ord","mod","usn","type","queue","due","ivl","factor","reps","lapses","left","odue","odid","flags","data"],"rows":[]},
"revlog":{"columns":["id","cid","usn","ease","ivl","lastIvl","factor","time","type"],"rows":[]},
"graves":{"columns":["oid","type","usn"],"rows":[]},
"deck_config":{"columns":["id","name","mtime_secs","usn","config"],"rows":[]},
"config":{"columns":["KEY","usn","mtime_secs","val"],"rows":[]},
"tags":{"columns":["tag","usn","collapsed","config"],"rows":[]},
"fields":{"columns":["ntid","ord","name","config"],"rows":[]},
"templates":{"columns":["ntid","ord","name","mtime_secs","usn","config"],"rows":[]},
"notetypes":{"columns":["id","name","mtime_secs","usn","config"],"rows":[]},
"decks":{"columns":["id","name","mtime_secs","usn","common","kind"],"rows":[]}
},"media_sha256":{}}
```

`media_sha256` is the whole exact Unicode filename→64-lowercase-hex digest map. Each entry is a regular file; non-Unicode names/directories/symlinks/special entries fail. Require existing package filename safety separately; this does not pretend the old serializer already enforces that wider policy. Map keys/digests must agree exactly with result `media`, complete package/destination bytes and lengths. JSON object order/whitespace do not define equal content; retain/hash actual serialized bytes rather than claim canonical JSON byte equality.

At this **new** boundary, strictly validate exact envelope/table/column/cell sets, lengths, integer range, hex spellings, all twelve complete tables, duplicate JSON members, row-key uniqueness and required row order, map safety and full CAS/byte agreement. Existing producer emission facts are not proof of these input refusals. Reject unsupported cells/schemas; preserve original package/database on failure. `package_db_dump(raw=true)` is a different format and cannot substitute for these rows.

## M6. Separate offline effects, freezing and unchanged acceptance

Two independent offline comparisons are required: **frozen B authority→decoded exported package** for export, and **original decoded Mobile database→closed fresh SDK destination** for import. Retain every original/raw/post-operation state and full delta. By default no offline cell or media delta is permitted. A later focused 26.09.4 qualification may establish a finite independently reviewed operation-specific effect (exact fields and acceptance rule) before authority admission. Unknown initialization/stamp/schema effects remain unresolved; no blanket mod/USN/SCM allowance. No sync runs here, so existing outer sync/protocol-stamp masks excuse **no** offline change. Compare every other cell and media byte exactly. Do not infer allowed effects from candidate output or normalize raw artifacts.

Static recipes/rules/code/initial inputs are independently reviewed before final execution. Under those reviewed L1 rules, freeze each actual Mobile-origin package, original database/snapshot, conversion, case/action/actor/baseline/review and capture order before that event's first candidate submission; independently review actual artifacts before credit. A post-receive witness is only its declared checkpoint. Converter bytes cannot establish installed device identity or timing without the independently admitted actual staging evidence.

Success requires complete closure resolution and trusted source/image/runtime/run/selection/recipe/review/baseline/action/staging agreement, both raw states and snapshots, whole media equality with payloads, and the operation-specific effect proof. Missing/partial/stale/swapped outputs or unsupported schemas fail. Actual MG receipts remain separate. No helper lane, outcome, seal, synthetic receipt or extra case is created.

Keep outer schema actions/spec whitelist unchanged and cases bound to `conformance/meguri-sync-server#C12`; this preparation uses existing artifact roles and observation envelopes only. Any later addition of this new spec ID to the outer whitelist requires explicit narrow review. Preserve ten cases/22 actions, both collection/media directions, fresh receiver, explicit replacement authority, rich intermediate states, real reviews, no-change and actual device/build/OS/receiver proof. Exact `.txt` add/same-name replacement/selective delete assertions remain unchanged. No PNG mapping, unsupported UI claim, import-as-media-update workaround or waiver is authorized.

Implementation requires qualified session/source ownership and focused currentSDK public API/options/bounds/effects qualification. Before any staged Mobile authority is admitted, the finite actual device/export/offline evidence resolver in M3 also requires independent review. Mobile same-name media authoring remains separately unproven. Snapshot grammar/order defines the existing first-party neutral format; old readers are not thereby claimed to enforce the new admission rules. This contract supplies no code, runtime, package, device/service or compatibility result.
