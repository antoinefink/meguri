# U28 — all-name query for Go to Deck

Supplemental starting commit: `1df3bcea29004e49565d0bd002c076aff180ddeb`.
Intake: `build/unit-starts/U28-all-deck-names-20261026.json`.
The U156 start is recorded separately. The gate base is still
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The scoped API/prepared-work audit finds no complete exported name list.
Display trees hide empty Default and omit missing-parent records. D17 adds
a separate generation-tagged, read-only projection of all persisted records;
the display-tree contract is unchanged.

Spec-writer session `s-20261026T041939Z-a8f043ce` uses only public Meguri
contracts. No reference, oracle, private or Anki source is read. Independent R1
clears the substance and requests R26/R27 in the Requirements header. That
metadata fix is the sole second edit; R2 closes it with no further finding.

Implementation, focused query/generation/purity checks, Swift bridge checks
and the native switcher remain pending. No unit or integration acceptance is
claimed by this spec addition.
