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
