# Provider restoration after cancellation

Starting commit: `7b2af24eaa444509751527313f29cfff24168a29`.
The original app gate base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`; that gate is failed.
This repair has no unit or release acceptance yet.

The whole Host publication binary in `core-test/20261117T102842Z-23892`
passed 134 methods and failed three. After accepted replacement setup,
cancellation restored the predecessor tracking rows but retained the candidate
provider association. Later reconciliation or reopening rejected that mismatch.
The three original failing test bodies remain unchanged.

The repair keeps the durable Cancelling operation until the exact prior fence,
provider mapping and final tracking transaction have settled. An interrupted
write remains uncertain even if its rename is visible. Cancellation republishes
matching bytes through the existing guarded write before deleting its evidence.
A conflicting handoff or successor retains the operation. Provider publication
ownership excludes setup and qualification during the transition; provider I/O
runs outside Runtime and tracking guards.

Stopped, signed-out and account-changed predecessors regain metadata only.
Their writers remain closed until a fresh Resume proves activation. Cancellation
of an initial publication or a local-only replacement reopens discovery without
inventing Connected consent. Sixteen additional Host regression entrypoints
cover these boundaries, failure retry, cold recovery and contention. The first
execution is recorded below; source review alone grants no test evidence.

Filtered author exports `103929Z-60015`, `111436Z-50735` and
`114904Z-84769` retain the successive repairs. Independent reviews
`105548Z-11644` and `113133Z-16182` retain their findings. Final independent
review `cleanroom/20261117T121757Z-61455` finds no P0 or P1 in the unchanged
Round 3 candidate. The actual final Git patch SHA-256 is
`93a3a0d16b13bbf1e7dff929da41fe4545431e6d3e0a5dc833979573e2e9e8a2`.
The recorded author session is `s-20261110T214025Z-20fa15fd`; the non-author
review session is `s-20261109T210639Z-47d06d5b`.

One optional P2 remains: a non-resumable predecessor such as ReplacedLocally
or ZoneMissing can require a provider refresh or reopening before another
connection attempt. Local data remain retained and writers closed. Direct
Connected-provider post-rename failure and further successor interleavings also
have optional coverage gaps. These are not claimed as verified behavior.

The final reviewer read all contents of all four files, including comments,
literals, data and paths, and classified them PUBLIC with no new authorship or
license concern. Root verified the exact applied bytes:

| Path under core/crates | SHA-256 |
|---|---|
| meguri-sync/src/provider.rs | bf6c482a6af4119659204ecb1bb0b62ee6751dc7403bce920ad1fac271114358 |
| meguri-sync/src/cloud/runtime.rs | 93aeb1d237616bd4100c55742db9eb75b7ea5e5a16a45f3f97e73be7f868770c |
| meguri-sync/src/cloud/publication_runtime.rs | 8d7f66feca1f6cc38c09bff28058639440351fefff38d7c7bacdc5dff2afc8f6 |
| meguri-host/tests/cloud_publication_phase.rs | 3c025e50d4b1c1a36d16fb4dac845b9d4dece2b21427ff271296cbb800476992 |

Ordinary privacy `122742Z-86504` flagged only the provider and Host test files.
Under the standing routine-admission delegation, only those two exact public
hash entries were refreshed or added. Matcher behavior, tokens and access are
unchanged. The initial complete report was independently classified PUBLIC by
`/root/provider_test_delta_review`; only its exact hash was admitted after
ordinary privacy flagged it. The required integration checks remain due.

## First execution

Clean repair commit `0aa73cdbe` compiles. Focused
`core-test/20261117T123409Z-12376` executes 25 methods: 23 pass, two fail,
zero skip. All three original failures and the previous six consent and
restoration methods pass. Two new warm-cancellation cases fail at the same
explicit reconciliation after reopening. No whole-binary pass is claimed.

Independent source audit `/root/adoption_empty_decks` identifies the missing
arrangement: a fresh Runtime intentionally starts with unverified account
identity. Direct reconciliation requires that proof but does not read it.
The cold-cancellation branch already obtains it through actual cancellation.
The warm branch omitted the normal Host preparation port after reopening.
A bounded test-only correction adds that real preparation only to the warm
branch, before retaining the explicit reconciliation and all existing
assertions. No consent, verification flags, SDK instance or Active proof is
injected. Its execution remains pending.
