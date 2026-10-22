# RF2: safe local collection entry

Starting commit: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
Authority: Antoine's review-first ordering, recorded in
`docs/process/review-first-delivery.md`. RF1's full gate and catch-up passed.

This delivery adds supported local collection entry to the real review app.
Start with latest-format, collection-only `.colpkg`. Preserve the source,
inspect a staging copy, verify it and activate it through the planned replace
primitive. Existing local data requires a real safety backup before replacement.
Unsupported formats and media payloads must receive an explicit refusal.
No Apple account or network connection is required for this local path.

Reuse the prepared U124a package and replacement specifications and their
original provenance. Reconcile the obsolete replacement recovery section with
the current persistent generation/account publication fence before implementation.
Reuse the relevant prepared backup specifications for the mandatory safety path.
Keep the full U124a, U126, U127 and U166 acceptance requirements open until met.

One integrated delivery item: package and replacement foundations, real safety
backup, job/FFI service, security-scoped picker and persistent imported reviews.
Component commits retain focused checks and clean-room provenance. The final
Swift/FFI integration gate uses the starting commit above. The spec writer,
filtered implementer, independent reviewer and fixer sequence remains mandatory.
Review covers correctness, plan conformance, clean-room boundaries and UI quality.
No Unit trailer or completed implementation is claimed at this starting record.
