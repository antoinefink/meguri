# Exchange-incomplete contract draft

Status: Draft; independent review remains open. No implementation or test result is claimed.

Role: spec-writer. Session: `s-20261113T153228Z-f5b77207`.
Starting commit: `ee5bfe364f881f3ed890faa2fba067a6f257812e`.
MG recorded the session in `053c73b7b81d9767d221071e91a712b81e49703c` before these edits.

P12.6.1 keeps durable facts about accepted incomplete work. Other results keep that work; another incomplete adds work. Outgoing work uses its original P12.4 boundary. Incoming work counts done through durable apply, a K13 accepted version or deletion that settles the original, or a durable reason change to generation, reference or structure. Other attention, counts, holds, apply and recovery rules still apply.

The item has count 1, also for an entity with work in both directions. A later success clears earlier work only with durable accepted proof of the original exchange order. Device time, delivery order and result-write order alone prove no order. This covers first late delivery as well as repeated results. Unknown older work or order stays unknown. Rust must version the new facts and chooses the physical schema.

Only P12.6.1 and the six new P21 bullets are added to the Under review (R2066) scopes. Existing status scopes remain. AE91 needs no edit. P24 remains open. The new section defines when incoming work counts done for this item.

Independent review and privacy checks remain open. Review must check exact original versions, durable facts across reopen, guarded original exchange order and unknown older data. No source implementation, build or test was run for this draft. The prepared edits are not committed.
