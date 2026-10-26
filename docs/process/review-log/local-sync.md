# Persistent local sync utility

> **2026-10-26: Anki server work is frozen indefinitely.** Earlier next steps
> and acceptance requirements below are retained history. Do not resume them
> without Antoine's explicit request. See [the delivery scope](../review-first-delivery.md#anki-server-freeze--2026-10-26).

Start: `63d496525bf83d3b3afbb747c36971fb8d161f61`.
Implementation session: `s-20261024T173251Z-cb356af3`.
Fixer session: `s-20261024T174115Z-511e3ad6`.

This separately owned Docker service preserves its data and private configuration
when stopped. It uses an installed immutable runtime image. It is independent
of the disposable automated sync rig. Runtime values remain in ignored files.

## Review and fixes

Independent round 1 found interrupted volume initialization could prevent later
startup. The fixer made root ownership initialization repeatable without touching
existing files. Round 2 found no P0–P2 issues.

Runtime verification then found Docker could change an automatically allocated
port after restart. The fixer now selects and persists the port before container
creation, binds it explicitly, and refuses inconsistent retained bindings.
Round 3 found no P0–P1 issues. The final seven focused cases passed in
`selftest/20261024T175508Z-81267`; they cover foreign resources, retained state,
interrupted initialization, fixed bindings, durable writes and error redaction.

## Runtime evidence

- Startup: `local-sync/20261024T174726Z-75012` passed.
- Initial production Rust authentication and strict metadata decoding:
  `local-sync-check/20261024T174840Z-76009` passed.
- The stopped, empty initial container was replaced without removing its data
  volume: `local-sync-rebind/20261024T175703Z-83296` passed.
- Corrected stop/start retained a byte-identical private client file.
  Production Rust authentication and metadata decoding passed again in
  `local-sync-check/20261024T175943Z-86487`.
- Seed verification: `source-seed/20261024T174118Z-70423` and
  `source-seed-target/20261024T175958Z-86945` passed. A fresh target download
  contained 1,001 cards, 1,001 notes and 2,954 media files. All media filename
  and SHA-256 pairs matched; total media size was 126,856,239 bytes.
- Production Rust authenticated and decoded the populated server metadata in
  `local-sync-check/20261024T181009Z-90965`: nonempty collection, media USN 2,954.

The earlier port failure remains retained; it is not a passing receipt.
These checks do not prove native app import, full collection field fidelity or
milestone acceptance. The required integration gate remains pending, with base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf` covering the pending sync work.

## Final tooling checks

Static checks `20261024T181448Z-92934` and similarity
`20261024T181505Z-93757` passed. Final helper self-tests
`20261024T181604Z-95391` passed all seven cases.

Privacy scan `20261024T181507Z-94009` reported two Docker command lines.
Antoine approved only the exact quoted public option spelling
"no-new-privileges" in escalation `E-20261024T181932Z-5d69`.
The scanner excludes only that complete literal span. Bare, modified, surrounding
and crossing matches remain checked, including decoded and binary inputs.
Independent review found no P0–P2 issues in this addition. Scanner tests
`selftest/20261024T181905Z-291` and the three-file privacy check
`private/20261024T182001Z-1947` passed.

The first similarity and doctor attempts could not reach Docker from the
restricted sandbox. Authorized runs verified OrbStack and the running test VM;
doctor `20261024T181509Z-94302` reports no required setup missing, 89 GiB free,
and the existing host/VM Xcode version warning. Catch-up
`20261024T181623Z-95724` refused the uncommitted tree and remains pending after
the tooling commit. No failed or unrun check is reclassified as passing.
