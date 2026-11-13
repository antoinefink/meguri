# Exchange-incomplete contract review

Status: Only P12.6.1 and the six new P21 bullets are Approved 2026-11-13 (R2066). No implementation or test result is claimed.

Role: spec-writer. Session: `s-20261113T153228Z-f5b77207`.
Starting commit: `ee5bfe364f881f3ed890faa2fba067a6f257812e`.
MG recorded the session in `053c73b7b81d9767d221071e91a712b81e49703c` before these edits.

P12.6.1 keeps durable facts about accepted incomplete work. Other results keep that work; another incomplete adds work. Outgoing work uses its original P12.4 boundary. Incoming work counts done through durable apply, a K13 accepted version or deletion that settles the original, or a durable reason change to generation, reference or structure. Other attention, counts, holds, apply and recovery rules still apply.

The item has count 1, also for an entity with work in both directions. A later success clears earlier work only with durable accepted proof of the original exchange order. Device time, delivery order and result-write order alone prove no order. This covers first late delivery as well as repeated results. Unknown older work or order stays unknown. Rust must version the new facts and chooses the physical schema.

Independent review `cleanroom/20261113T153832Z-21327` covered immutable draft `7b7b89b7a76884250c3a7d3ab21141e293928df7` as its exact `main_input`. The report found no P0–P2 findings, returned a zero-byte patch, and allowed approval of only P12.6.1 and the six new P21 bullets. The Status suffix records that approval. Existing status scopes, AE91 and P24 remain unchanged. The section and test text remain identical to that reviewed draft.

The first privacy scan, `private/20261113T153434Z-20021`, failed on the new prose. The paragraphs and test bullets were rewritten; `private/20261113T153746Z-20636` passed the rewritten draft. Neither result is replaced by this log. The approval delta passed `private/20261113T154306Z-24790`. No source implementation, build or test was run for this spec work. No P24 or gate completion is claimed.
