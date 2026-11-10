# specs/servers/migaku-routing.md (U138)

Spec-writer session s-20261110T085429Z-165d83d3. Written in Meguri's own
words; no code or regular expression of the Migaku add-on (field-type
inference) or of the user's local hook was copied or translated, and those
sources were not read in this session. No note content from the user's
collection appears in the spec: every example sentence and annotation is
invented for the spec (D8's two-alternative example was replaced by an
invented one after review, because the earlier one matched field data quoted
in the research). The cut-off date of the received notes (D14.1) is a private
value of U249's run configuration and is not in the spec.

Anki 26.09.3 (`refs/anki-26.09.3`): not needed. Collection facts (note ids as
creation times, card decks and original decks, field ids, usable decks,
unicase) are cross-referenced to existing specs.

| Source | Read for | Spec sections |
|---|---|---|
| `local-data/research/09-ankiconnect.md` §3.2 | payload keys and item shapes; the add-on's destination choice (Add window, else configured note type and deck), no automatic routing; derived sources; inferred map of the 11-field Custom types; the local defaults hook and its per-note-type flags; POS tags seen per language; destination decks | D1, D3, D4, D5, D6, D14.2, D16 |
| `local-data/research/09-ankiconnect.md` §0 | every Custom note created since the log's first `/receive` creation came through `/receive`, destination decks; the dossiers disagree on when the log began (09 header and 01 §9 give its first line, 09 §0 the first creation), recorded as D17 item 7 | D14.1, D17 |
| `local-data/research/09-ankiconnect.md` §3.2, §3.3 | the sample payload with `{、}` raw marks; `{…}` marks raw text; the `word[lemma,pos,gender\|alt…]` shape | D8, D10 |
| `local-data/research/01-user-collection.md` §4, §8 | Migaku syntax examples for JA and DE/ES (shapes only, no example reproduced); six Migaku note types, the stock ones with 0 notes; the defaults hook | D6, D8, D10 |
| `local-data/research/12-template-compat.md` §1 | Migaku Custom vs stock note types and note counts | D6 |
| `specs/servers/migaku-receive.md` P9, P12, P14, P20 | MIME and first-byte extension rules the rebuilt media must satisfy; retry matching and its 600 s window; Created rows | D14.2, D14.3, D17 |
| `specs/servers/migaku-received-log.md` L4 | no `Uncertain` decision; every `Fallback` row is an uncertain card; the label is derived from the decision and the detected language | D11, D13.1, D17 |
| `specs/servers/host-security.md` H10.1, H20, H21, H22 | integrations store, FFI module, clocks, test construction and socketless serving (consistency pass of 2026-11-10) | D1, D2.1, D14.3, D18 |
| plan, Product Contract R1692, R1693, R1703, R1710–R1720, R1722, R1723, R1947; AE71, AE87; U138, U139, U140 | requirements, decision kinds, corpus tooling, Fallback recording rule | all |

Meguri decisions made here (no external source): the language table; D4's
English keyword rules; D6's 90-day window, tie rules and naming; D7's
message strings other than `No fields to map to.`; D8's preparation; D9's
code-point classes and the Han rule for Chinese profiles; D10's stopword
lists, letter lists, verb tags and per-occurrence counting; D11's tie rules
and inclusive 60-minute window; treating a confident language with no
profile as uncertain (from U138's Approach and test list); D12's update
rules; D2.1's save errors, the loudness range −40 to −10 and the effect of
edits; D2.3's operations; D2.2's id rule; D8's ASCII-letter tag rule and
invalid-reference rule; D14's extraction, replay options, placeholder media,
buckets, report shape and gate reading (R1718 read as no wrong language at
all, R1947/AE87's flags read as the profile's defaults); D15.2's corpus
rows and clock values; D18's expectations on `servers/host-security`. They are listed as open questions in D17 where a source
might later contradict them.

Privacy pass (2026-11-10): `tools/mg private scan-outputs` flagged words
that the owner's collection holds and no committed file does. D10.1 lost
the flagged stopwords and gained short ones that pass (D17.11); the D8,
D10.2, D15.1 and D15.2 examples use other synthetic words with the same
scores; D1 names the `it` language through ISO 639-1. The corpus file
`conformance/corpora/migaku-routing-synthetic.jsonl` was written from D15.2
after that pass.
