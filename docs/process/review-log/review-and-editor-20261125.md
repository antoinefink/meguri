# Review controls and Mac note editing

The review footer must keep its existing controls legible over a long card. The finished screen's Space and Return keys should run Done, and R should replay the current card side. On Mac, Add Note should use the same window layout as Edit Note with selectable note type and deck.

## Browse regression

The storage reader sorts fields by stored ordinal and preserves each row's number. The editor rejects any layout whose stored numbers differ from the fields' array positions. This includes imported layouts with gaps, which `notetypes/records` N1 supports. A regression test must edit the later field in such a layout and verify that the earlier field keeps its contents.

## Session record review

An independent reviewer (`metadata_review`) read both complete session records, including their paths, and admitted their contents as public operational metadata.

| Session | SHA-256 |
| --- | --- |
| `s-20261125T204705Z-a4cccc69` | `842b74e1b4a9b559907781493b1f5d49df10eb41d49010a4e44b7ad68d5f0362` |
| `s-20261125T204711Z-ce6a10fc` | `a649400bfa0eb037a5f04e6c14bd55afd50bc121872bce1d6aae286d2ffcd291` |
