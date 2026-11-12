# Epoch crt Unknown contract packet

Author session: `s-20261112T194140Z-9beb6ca5`, role `spec-writer`. The author prepared an ignored patch without editing canonical files. Integration follows independent review at master `b1103527`.

Independent reviewer: `epoch_contract_review`, session `s-20261112T203107Z-3e31fac1`. The first review found no P0/P1 or owner/product decision. Its optional P2 asked for the encrypted-field boundary to distinguish absent `e`, invalid present bytes and failed CloudKit reads. The patch now assigns these to waiting, refusal and P15 respectively. The same reviewer confirmed the delta clear: no P0/P1/P2 remains.

Approved 2026-11-12 (R2066) applies only to the new P6 row and paragraph "Unknown epoch crt", P12.3 paragraph "Epoch crt preparation", and V26.1 paragraphs "Sync Now epoch verification". The final packet change updates only their appended Status clauses. Original Status text, including the P24 K2 Under review exclusion, remains byte-for-byte.

The patch adds one guarded direct bound-epoch read before the Sync Now gate check when absent authoritative epoch `crt` is its sole blocker. It preserves the separate read purpose, validation guards and existing evidence requirements. No implementation, test or acceptance result is claimed.
