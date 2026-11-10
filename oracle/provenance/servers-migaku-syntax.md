# Migaku bracket syntax (U139)

Spec-writer notes for `specs/servers/migaku-syntax.md` S1-S10 (session
s-20261110T085429Z-165d83d3). No Anki, AnkiConnect or Migaku add-on code was
read: the Migaku add-on and the extension bundles are outside the sources this
role may use for this wave, so every rule comes from the plan and the private
research dossiers' prose, and where those are silent the rule is Meguri's own
and is flagged as an open question in S10. No user note content was copied; all
examples are synthetic and were checked against the dossiers so that no token
of the user's cards or payloads appears in them.

Versions: the dossiers describe the Migaku add-on 0.6.6 (its derived-value
rules and the card scripts it writes into the Migaku note types) and payloads
of the extension bundles 1.30.16.0 and 1.31.0.17.

| Spec section | Source | Version | Read for |
|---|---|---|---|
| intro | plan R1693, R1712, R1716 | Meguri design | what the three values feed; that R1712's map uses none of them |
| S1 | plan R1716 (character classes); `specs/servers/migaku-routing.md` D9 | Meguri design | the script classes, reused for the family choice |
| S2 | `local-data/research/09-ankiconnect.md` §3.2 (payload example), §3.3 (Japanese and German/Spanish card scripts); `local-data/research/01-user-collection.md` (Migaku Custom section); `local-data/research/12-template-compat.md` (Migaku rows) | add-on 0.6.6 card scripts; extension 1.30.16.0, 1.31.0.17 payloads | the observed forms: `surface[reading,dictionary form;pitch]`, pitch letters, `{raw}`, spaces between Japanese tokens, `word[lemma,pos,gender|alt]`, `(word)[…]`, `<t>` rewritten to `<strong>`. The readings "Japanese text has no spaces of its own" and "a parenthesized surface may hold several words" are Meguri's (S10 Q3, Q6), not observations |
| S3, S4, S5 | plan R1693 ("separate rules for CJK, Korean and European text") | Meguri design | that three families exist; the recognition and stripping rules themselves are Meguri's (Q3-Q6) |
| S6 | plan R1693 ("readings inside `[...]`, pitch suffix after `;` dropped", "the IPA part"); `local-data/research/09-ankiconnect.md` §3.2 step 2 (the IPA as the third comma-separated part) | add-on 0.6.6 | the reading rules; the conflict between "third part" and S2's European form is recorded as Q2, the joining rule as Q1 |
| S7-S9 | Meguri design | Meguri design | invariants and tests |
| S10 | the rows above; plan R1714 (default profiles, none Korean) | add-on 0.6.6 | which add-on outputs are not known: Q1 CJK reading, Q2 European reading, Q3 spaces, Q4 Korean, Q5 European braces, Q6 parenthesized surfaces |
