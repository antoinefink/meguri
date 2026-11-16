# Meguri Sync — neutral server contracts

Plan: [Meguri Sync Server](../../plans/2026-11-15-1028-feat-meguri-sync-server-plan.md), unit U2.

Starting commit: `ad5a9474907d2380aa9cbeeeae10e80da485be79`.
Spec-writer record: `s-20261115T112728Z-bd077662`, recorded after the contracts were written;
its note preserves the original base. The ledger commit is `0b53b045`.

The contracts cover native transport, collections, publication and recovery,
media, browser accounts, and neutral interoperability evidence. They distinguish
existing neutral baseline facts, original service policy, and unverified
current-client probes. The 26.09.3 baseline supplies no 26.09.4 or Mobile pass.
Original browser policy can be implemented independently of those probes.

The writer used existing neutral specifications and the agreed Product Contract.
No Anki implementation, oracle source, private collection, or raw reference
report was read. No production application or server code changed in this unit.

An independent spec-writer review found that a sealed manifest could omit the
client/server lane distinction and could substitute a service for the required
Mobile target. The correction adds structured peer identities, applicable
runtime/fixture hashes, explicit target constraints, and required review,
observation and MG artifacts. Mobile uses exact app/OS builds and bound device
evidence; extracting its executable is not required. The harness must also
validate counts, case IDs, outcomes, and artifact/source bindings. The independent
follow-up found the correction complete and no remaining actionable finding.

Verification so far is an independent document/schema review. No target runtime
or interoperability test has run, and no current-release behavior is sealed.
Schema execution belongs to the reference harness unit. The first staged privacy
scan, `build/mg/private/20261115T112855Z-74479/result.json`, failed on vocabulary
matches. Owner-authorized maintenance refreshed only previously public words and
added reviewed exact-content technical-document entries. The later scan
`build/mg/private/20261115T130054Z-93359/result.json` reports no contract failures;
its two review-log wording matches were corrected. The follow-up
`build/mg/private/20261115T130305Z-93755/result.json` passes for both logs.
This unit supplies neutral contracts, not runtime or integration acceptance.
