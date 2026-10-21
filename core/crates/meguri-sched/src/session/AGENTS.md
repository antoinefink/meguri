# Session values

- Production and E2 compatibility share `SessionQueue`; only production marks a current card. A compatibility read never reserves admission or selects a deck.
- P9's learning-count/cutoff snapshot precedes cutoff advance. Redo restores that snapshot, not the answer instant. Undo can break learning-key order at the front.
- State tokens consume exact stored data bytes; CardData re-encoding is not a token input. RowDigest remains the separate writer precondition.
