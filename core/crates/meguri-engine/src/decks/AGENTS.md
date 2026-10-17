# decks/

- `save_in` is the path for a deck edited by another area: it normalizes a stored name and performs the subtree rename when normalization changes it (D13). Editing one blob starts from the live row so unknown fields and original bytes survive.
- `delete` calls the backup service off the writer and guards its summary with deck/card table preconditions; `delete_in` is the transaction primitive and the caller supplies any required backup. The conformance op deliberately excludes the backup (K5).
