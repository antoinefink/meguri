# Candidate invocation files

This is original test policy under `conformance/meguri-sync-candidate#K1–K4`.
It establishes no candidate, release-package or Mobile result.

The MG wrapper creates the candidate and joins its network namespace before invoking the quarantined client entrypoint:

```text
candidate --input /in --credentials /secret/credentials.json --phase full
```

`full` selects 11 Desktop transport and full-transfer cases.
`complete` selects the 21-case Desktop smoke contract, including normal sync and media.
The upstream is always `http://127.0.0.1:8080/` inside that namespace.
No fixture selects a URL, command, executable or container.

Mount these inputs separately:

| Path | Content |
|---|---|
| `/in/peer.json` | Neutral peer/build receipt described below |
| `/in/artifacts/<sha256>` | Retained executable, lock, MG build receipt and execution-binding bytes |
| `/secret/credentials.json` | Readonly 0600 regular file owned by the client UID; exactly `username` and `password` strings |
| `/out/invocation.json` | Actual MG invocation with C11 source fields, executable hash, run ID and author session |

Never retain the credentials in fixtures, artifact stores, logs or command arguments.
The wrapper owns initialization and synthetic account setup. The adapter does not read server source or storage.

`peer.json` has exactly these fields:

| Field | Required value |
|---|---|
| `format` | `meguri-sync-package-v1` |
| `kind` | `intermediate_build`; this receipt grants no production-package acceptance |
| `server` | Existing schema `server_peer`, kind `meguri_sync`, with exact version/build/source/lock/executable identities |
| `source_dirty` | `false` |
| `profile` | `debug` or `release` |
| `features` | Sorted unique enabled feature strings |
| `executable_artifact_sha256` | SHA-256 of the retained executable bytes; equals the server identity |
| `lock_artifact_sha256` | SHA-256 of the retained lock bytes; equals the server identity |
| `mg_build_receipt_sha256` | SHA-256 of the actual retained MG build result |
| `build_invocation` | `job`, `run_id`, `arguments`, matching that build result |
| `execution_binding_sha256` | SHA-256 of the wrapper's checked execution binding |

The passing, non-dry MG build result must name the same clean source commit.
This initial producer accepts committed clean source only. A WIP source commit can provide that checkpoint without granting unit acceptance.
Its `details.candidate_build` contains exactly `server`, `source_dirty`, `profile` and `features`, matching the peer receipt.

The execution binding contains:

| Field | Required value |
|---|---|
| `format` | `meguri-sync-candidate-execution-v1` |
| `endpoint` | The fixed loopback endpoint above |
| `server` | The same complete server identity |
| `run_id` | Current MG execution run ID |
| `container_id` | Actual 64-character container ID created by this job |
| `labels` | Includes `meguri.mg.run-id` equal to the current run ID |
| `observed_executable_sha256` | Hash checked against the running owned binary, matching the build |

The wrapper must inspect the actual binary and ownership label before writing this binding.
The adapter checks the retained fields and hashes; it cannot inspect Docker from inside its container.
The container ID is evidence, never a selector supplied by a fixture.

The wrapper uses the existing MG result contract and publishes `details.reference_invocation` and `details.reference_evidence`.
Final MG counts must match every selected/completed case, with zero failures and zero skips before a seal.
Debug interoperability remains scoped to that exact build. Missing or stale inputs fail without a reference or public-server fallback.

## Exact local package selections

The historical intermediate_build table retains its scope. Final runs use
conformance/meguri-sync-local-delivery#L2–L3, binding actual package source,
image, running process, locks/features, assets and manifests; never relabel debug.

Desktop31 is the fixed concatenation of unchanged full11, normal10, media4 and
candidate-final-additions.json six cases. Mobile10 and retained8 use
candidate-mobile-final-selection.json and candidate-retained-final-selection.json.
final-fixture-recipes.json gives original independent values. Preparation freezes static
initial input manifests, IDs and expected rules before final execution and review.
Dynamic offline event snapshots follow L1 capture-before-submission rules, with
actual artifacts independently reviewed before credit. Source templates do not fabricate runtime/device identities.
Missing actual device/private routing remains incomplete. Historical selections
and research catalog retain their scope/counts.
