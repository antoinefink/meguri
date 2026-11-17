# Current-release sync reference

This test-only image pins Anki 26.09.4 separately from the application oracle.
It imports no production Meguri code. Its SDK adapter and reference dependencies stay in this AGPL quarantine.

Run every command from the checkout root:

```sh
tools/mg sync-server-reference selftest
tools/mg sync-server-reference identity
tools/mg sync-server-reference deps
tools/mg sync-server-reference catalog
tools/mg sync-server-reference smoke
tools/mg sync-server-reference edges
tools/mg sync-server-reference normal
tools/mg sync-server-reference validate build/mg/sync-server-reference/<smoke-run>
```

`lock` resolves the exact test graph in a separate Docker container.
The reference runtime has no external network route, published port, or checkout mount.
Clients and the reference server communicate over their container's loopback interface.
The source digest must remain stable throughout the build and execution.

The catalog lists obligations for all 31 families and applicable targets.
The smoke selection covers only its named Desktop observations.
A smoke pass does not complete a broad family or prove Mobile compatibility.
The catalog keeps all other obligations unobserved.
The separate edge selection covers exact media page boundaries, replay no-ops and a bounded idle observation.
Its zero-state case observes no listing request, not an empty server page.
Its abort injection tests expired-round handling, not automatic reference expiry.

Each run retains the selection, original synthetic JSON, fixture manifest, observations, and hashed artifacts.
The validator checks exact outcomes, counts, source identities, runtime identities, and retained artifact bytes.
Native session edge requests come from the local fault driver.
The report marks session header changes and the empty-sk response injection explicitly.
These requests do not claim to be unmodified Desktop traffic.

`smoke build/<request>.json` accepts a closed JSON request:

```json
{"format":"meguri-sync-request-v1","operation":"smoke","client":"desktop","peer":"reference"}
```

Malformed requests fail. A missing candidate fails. Mobile requires an actual device run and remains unavailable here.
This unit does not start a candidate server. The candidate integration wrapper owns that process and its checked build receipts.
Its fixed file-input conventions are `conformance/recordings/scenarios/meguri-sync-server/candidate-inputs.md`.
The `candidate` image entrypoint shares the actual SDK pipeline with the reference run and never starts a reference fallback.

To seal a successful smoke, an independent oracle maintainer must review its exact source and retained artifacts.
The reviewer supplies `review.json` in that run's directory, containing the accepted bindings for every selected case.
Use format `meguri-sync-review-batch-v1`, role `oracle-maintainer`, the reviewer's recorded session, outcome `accepted`, and `bindings`.
Then run `tools/mg sync-server-reference seal build/mg/sync-server-reference/<smoke-run>`.
The seal requires the actual successful MG receipt from clean committed source.
It cannot turn a mismatch, unavailable target, stale source, or failed execution into a pass.

Anki remains AGPL-3.0-or-later. See `../LICENSE`.
JSON Schema validation uses MIT-licensed jsonschema and its locked dependencies.
Raw protocol framing uses zstandard under its BSD license option.
These dependencies never enter a production target or package.
The dependency report checks the exact graph, published hashes, and reported advisories.

The separate `normal` mode selects twelve finite observations. It measures actual
Desktop edits, a scheduler review, deletion, authored objects/config/tags, tuple
and grave boundaries. Explicit auxiliary supported-protocol rounds measure cursor
selection, one deck's modification conflicts, note conflicts, ordered duplicate
notes/reviews, and successful sanity/finish stamps. Actual Desktop receivers read
those committed results. Auxiliary requests are never called SDK-emitted bytes.

These cases leave card/notetype/preset conflict matrices, casefolded object names,
config replacement/null semantics, failed sanity, same-clock rounds and lifecycle
fault/recovery families unobserved. They do not enable a candidate normal gate.
The mode uses the same semantic recorder and validator as smoke and edges.
The catalog command exports its exact selection and original fixture; the image
executes their quarantined definitions without changing the existing neutral inputs.

Reference invocations name their source authors explicitly. The wrapper verifies
those records in the executing commit's ledger and retains `source-authors.json`.
Checkout paths and the latest ledger row do not select an author. Import the
source author's session-only commit with this source change before execution.
The closing `ls` assertion uses the raw served-file stamp retained by the recorder.

`tools/mg sync-server-reference normal-matrix` runs seven bounded, valid synthetic
Desktop groups: card cursor/data, note type and authored preset timestamps, selected
ASCII deck/note type/tag names, optional configuration, configuration replacement
and outgoing creation time, and graves followed by same-ID occurrences. It keeps
normal12, smoke21, full11 and complete21 unchanged. It does not execute candidates.
The mode retains exact auxiliary request JSON/zstd bytes and responses in per-case
synthetic descriptors. Raw served schema-18 rows precede SDK normalization; native
server rows are read only after its job-owned process exits. `matrix-progress.json`
retains completed traffic and observations if the observer fails. Structural changes,
other Unicode spellings, complete P-families, Mobile and final acceptance are excluded.

The candidate `normal` phase selects ten real Desktop SDK exchanges from the
finite normal probe: no-change, reviews and deletions in both directions, note edits,
authored object/config/tag exchange in both directions, 250/502-row boundaries
and 251 card graves. It performs independent initial full setup per collection,
but the credited normal exchanges are inspected separately. It includes no
auxiliary-crafted normal matrix traffic, media, restart or Mobile claim.
`tools/mg sync-server-reference normal-sdk` exercises the same ten SDK paths
against a job-owned reference server. Actual candidate execution still requires
a committed clean build and the existing strict runtime/artifact validations.

`tools/mg sync-server-reference recovery-discover` records public SDK names and
signatures only. It never reads runtime source or documentation strings.
`tools/mg sync-server-reference recovery` runs three finite P-M07 reference
mechanisms: ahead, equal/reused and delayed/overtaken media histories. Each uses
stopped whole-root rollback, controlled replacement credentials, offline edits
and a review, equal collection modification with different schema stamps,
explicit authoritative full download, then public media reset and SDK sync.

The observation retains exact non-col user-table hashes, card/review rows,
configuration value bytes and all media hashes. Equality after a subsequent
normal sync permits only the observed config USN and modification-time stamp
changes; every other column stays in the semantic digest. Original development
failures remain in their receipts. The cases do not satisfy candidate-only
P-R01/P-R03, Desktop menu/dialog evidence, Mobile, or production restore policy.

`tools/mg sync-server-reference recovery-dirty` uses one equal-cursor rollback
fixture with an actual offline file addition, replacement and deletion, plus
collection editing and review. The initial exact-equality attempt remains red
in its original receipt. The final selection distinguishes the observed
in-place failure from the separate clean-receiver mechanism; the two results
are never interchangeable. The diagnostic attempt can upload the local-only
file, so the protected chosen server snapshot is reestablished before the
clean procedure. The closed dirty profile remains byte-identical in a separate
preserved directory. A new collection/media directory receives the chosen
snapshot through public SDK operations, without importing or synchronizing the
preserved dirty profile. Exact whole-file-name/hash equality, all non-col user
tables and card/review rows remain compared. No GUI procedure, candidate restore,
Mobile behavior or general dirty-media conflict rule is claimed.

Final Desktop delivery uses nine fixed `candidate --phase final-…` segments:
`full`, `normal`, `media`, `migration`, `replacement-upload`,
`replacement-download`, `metadata-only`, `fresh-fresh` and `rich`.
Each segment uses a fresh job-owned candidate instance and the existing protected
credential pair. The package image and outer MG identity remain identical.

`--stage execute` accepts only `final_package_preflight` from `/in/preflight.json`.
It retains raw observations and a non-credit admission. MG captures that
admission's hash, measures the actual after-state and supplies the complete final
receipt. `--stage record` makes no client requests. It binds unchanged observations
to that final receipt. `final-assemble --input /in` reads the nine fixed directories
under `/in/segments/` and requires their exact 31-case union.

The six additions freeze actual authoritative values before submission. Semantic
validation compares retained values and rows; claimed true booleans alone fail.
The migration source is isolated at fixed loopback port 8081. Its process and
cleanup evidence remain separate from the candidate at port 8080. These paths
provide no Mobile, retained-peer, GUI, deployment or broad catalog credit.

Source repair round 1 keeps the execute/record paths and nine segment CLI values
unchanged. Replacement media observations use new empty directories and cursors
before reconciliation and again at the final observation. Legacy final cases
retain actual actor/action events with capture, first-submit and completion order.
Each action binds its own inputs and original baseline. Rich mutations preserve
all untouched baseline values, including complete configuration values. Assembly
rejects reused candidate containers, creation nonces or data volumes. The owning
MG factory proves initialization and cleanup; its secret console output never
enters this evidence store.
