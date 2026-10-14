# Note GUID text

Spec: text/guid
Requirements: R771, R1458
Anki: 26.09.3
Scenarios: none (fixed table below; GUIDs Anki creates are random)

The text form of the GUID every new note gets (`notes.guid`).

## G1. Alphabet

91 characters, digit values 0 to 90 in this order:

```
abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_`{|}~
```

That is `a`–`z` (0–25), `A`–`Z` (26–51), `0`–`9` (52–61), then the 29 symbols `!#$%&()*+,-./:;<=>?@[]^_` + backtick + `{|}~` (62–90). The printable ASCII characters left out are space, `"`, `'` and `\`.

## G2. Encoding

An unsigned 64-bit integer is written in base 91 with the G1 digits, most significant digit first, without padding or prefix. Zero is the empty string. Every non-zero value takes 1 to 10 characters; values from 91^9 (427929800129788411) upward take 10.

## G3. New GUIDs

- A new note's GUID is a uniformly random unsigned 64-bit integer from Meguri's randomness seam (`meguri_model::rng`, R1875), encoded per G2. It is assigned when the new note is created and never changes afterwards (R771), whatever happens to the note's fields.
- Nothing checks GUIDs for uniqueness on add; the collision chance is negligible and there is no unique index on `notes.guid`.
- GUIDs that arrive by import or sync are stored byte for byte, whatever their form (R1458). Only Meguri's own new GUIDs follow G2.

## G4. Table

Implementer tests check G2 against this table (verified against the oracle):

| Value | Text |
|---|---|
| 0 | (empty) |
| 1 | `b` |
| 25 | `z` |
| 26 | `A` |
| 51 | `Z` |
| 52 | `0` |
| 61 | `9` |
| 62 | `!` |
| 90 | `~` |
| 91 | `ba` |
| 92 | `bb` |
| 8280 | `~~` |
| 8281 | `baa` |
| 427929800129788410 | `~~~~~~~~~` |
| 427929800129788411 | `baaaaaaaaa` |
| 4294967296 | `!5St/` |
| 1700000000000 | `c~MJPzD` |
| 9007199254740992 | `b]BKf,uBG` |
| 9223372036854775808 | `vYHz/gPN,i` |
| 12345678901234567890 | `C<D~1=[N#b` |
| 18446744073709551615 | `Rj&Z5m[>Zp` |
