# Accepted sync request and later review session

The spec writer and an independent contract reviewer resolved the P12.2
assumption about review at capture. P12.1 rejects a new request during review.
P12.3 permits a session to start during an accepted exchange. Preparation and
gate waiting can now precede capture, so those rules needed explicit wording.

P6, P7, P12.2 and P12.4 still apply in full, including the existing publisher
case. Preparation applies no collection data. This is a wording clarification;
it adds no acceptance evidence.

Writer: `crt_unknown_contract`, session `s-20261112T194140Z-9beb6ca5`.
Independent reviewer: `epoch_contract_review`, no P0 or P1 findings. The exact
reviewed text includes the existing acknowledged publication exception.
Both used Meguri contracts only and changed no implementation. Root integrated
the text after the active native run ended. Other review holds remain.

This record approves the clarification under R2066. It does not claim that the
latest implementation, affected tests, integration gates or signed round trip
pass. Their results remain in the separate implementation review log.
