# Migaku bracket syntax: stripping and readings

Spec: servers/migaku-syntax
Requirements: R1693 (`targetWordNoSyntax`, `sentenceNoSyntax`, `reading`); supports R1722 (stripped target word and sentence snippet), R1729 (duplicate key)
Anki: 26.09.3 (no Anki behavior: these rules are Meguri's reading of the Migaku interface)
Scenarios: none in `conformance/` (pure functions; Meguri's unit tests take S8's tables as their cases, see S9)

The Migaku browser extension (versions 1.30.16 and 1.31.0.17, the ones `servers/migaku-receive` targets) sends the target word and the sentence with word annotations in brackets, the "Migaku syntax". This file defines the three derived values that `/receive` offers as mapping sources and that the received log and the duplicate warning use: the target word and the sentence with the syntax removed, and the reading of the target word. The Migaku Anki add-on 0.6.6 is the compatibility reference for these values, but its exact rules are not known. Every rule below is normative for Meguri. Where the add-on's output is not known, the rule is marked "Meguri rule" and S10 lists the open question.

The 11-field Custom field map of R1712 maps none of these values, so they reach a note only when a profile maps them on purpose. They are also shown in Recently received and used for the duplicate warning. A difference from the add-on therefore changes display and warnings, never routing. Language detection reads the raw text (`servers/migaku-routing`).

## S1. Terms

- **Value**: one JSON string of the payload (`targetWord` or `sentence`), HTML included.
- **Tag**: a `<` and every character up to and including the next `>`. A `<` with no later `>` is text.
- **Text segment**: a maximal run of the value that holds no tag. The syntax is recognized only inside one text segment, so an annotation never spans a tag.
- **White space**: a character with the Unicode White_Space property (`text/unicode-primitives`).
- **Kana**: U+3040–U+309F, U+30A0–U+30FF, U+31F0–U+31FF, U+FF66–U+FF9F, U+1B000–U+1B16F.
- **Han**: U+3400–U+4DBF, U+4E00–U+9FFF, U+F900–U+FAFF, U+20000–U+2FA1F.
- **CJK punctuation**: U+3000–U+303F. **Full-width form**: U+FF00–U+FFEF.
- **CJK character**: a kana, Han, CJK punctuation or full-width form character.
- **Hangul**: U+1100–U+11FF, U+3130–U+318F, U+A960–U+A97F, U+AC00–U+D7AF, U+D7B0–U+D7FF, U+FFA0–U+FFDC.

These are the classes of `servers/migaku-routing` D9, which R1716 names for language detection. If that list changes, this one follows it.

## S2. The syntax

The Migaku syntax has these forms:

| Family | Form | Meaning |
|---|---|---|
| CJK (Japanese) | `surface[reading,dictionary form;pitch,pitch…]` | readings and pitch for the characters before `[`; the reading may be empty (`[;a]`), and the part after `;` is a list of pitch codes (h, a, n, o, k, with an optional mora number) |
| CJK (Japanese) | `{text}` | raw text, such as punctuation, which the card script does not parse |
| CJK (Japanese) | a space between annotated tokens | separates the tokens. Whether the text before annotation had a space there is not known; S5 step 3 removes such spaces (Meguri rule, S10 Q3) |
| European (German, Spanish) | `word[lemma,pos,gender]` | dictionary form, part-of-speech tag and gender of the word before `[` |
| European | `word[lemma,pos,gender\|lemma,pos,gender]` | alternative analyses, separated by `\|` |
| European | `(word)[…]` | an annotation of a surface in parentheses. S5 lets that surface hold spaces, so that one annotation can cover several words (Meguri rule, S10 Q6) |
| Korean | not known | the add-on has a separate rule for Korean text; its form is not known (S10 Q4) |

The extension rewrites its own `<t>…</t>` emphasis to `<strong>…</strong>` before sending, so values may hold `<strong>` tags around annotated words.

## S3. Choosing the family of a value

The family is chosen for each value on its own, from the characters of its text segments:

1. a Hangul character and no kana: **Korean**;
2. otherwise any CJK character: **CJK**;
3. otherwise **European**.

`targetWord` and `sentence` of one payload may fall in different families. The choice never depends on the destination profile, so `/create`, moves and the log give the same result for the same text.

## S4. Annotations

Candidates are found in the original text of a segment, before anything is removed. In CJK text, the spans of raw groups (S5 CJK step 1), braces included, are left out of the search: a `[` or `]` inside a raw group belongs to no candidate. A `[` at some position starts an **annotation candidate** when:

- a next `]` follows it in the same segment (no other `[` lies between them: the candidate ends at the first `]`), and
- the character just before the `[` in the original text is in the same segment and is not white space. The closing `}` of a raw group counts as such a character.

The candidate is the `[`, its content and the `]`. Which candidates are annotations depends on the family:

| Family | Annotation |
|---|---|
| CJK | every candidate |
| European, Korean | a candidate whose content holds at least one `,` |

Candidates are found from left to right. After an annotation, the search goes on after its `]`. A `[` that starts no annotation is ordinary text, and the search goes on with the next character. Requiring a comma in European text keeps ordinary brackets such as `Name[1]` or `word[sic]`. A bracket after a space, such as `Text [sic]`, is never an annotation in any family.

## S5. Stripping (`targetWordNoSyntax`, `sentenceNoSyntax`)

`targetWordNoSyntax` is `targetWord` stripped, and `sentenceNoSyntax` is `sentence` stripped. Tags are copied unchanged. Each text segment is rewritten as follows.

**European and Korean** (Meguri rule for Korean, Q4):

1. Every annotation is removed: its brackets and its content.
2. When the character just before a removed annotation is `)`, look back in the same segment for the nearest `(` such that no `(` or `)` lies between it and that `)`. If one exists, both parentheses are removed and the text between them stays. Otherwise the `)` stays.
3. Nothing else changes: braces, spaces and every other character stay.

**CJK**:

1. Reading from left to right, a `{` that has a next `}` in the same segment, with no other `{` between them, starts a **raw group**. The two braces are removed and the text between them is copied unchanged. That text is **protected**: no annotation is recognized in it, and its spaces are never removed. A `{` with no later `}` is ordinary text. Raw groups are found first, on the original text, and S4 then searches the rest of that same original text.
2. Outside raw groups, every annotation is removed (S4). Steps 1 and 2 both act on the original text, so their order does not matter.
3. Space joining (Meguri rule, Q3): in the result, consider each maximal run of U+0020 SPACE that is not protected. Find the nearest character before it and the nearest character after it, skipping tags; protected characters count. When both exist and both are CJK characters, the run is removed. Otherwise it stays. Spaces between Latin words of a mixed sentence therefore stay.

The result is HTML in the same sense as the input: entities are left as they are (`&nbsp;` is text, not a space for step 3).

## S6. Reading (`reading`)

`reading` is derived from `targetWord` only. When `targetWord` has no annotation, `reading` is the empty string.

| Family | `reading` |
|---|---|
| CJK | for each annotation, in order: its content up to the first `;` (or all of it when there is none), then up to the first `,` of that part. The non-empty parts are joined with no separator. |
| European | for each annotation, in order: its first alternative (the content up to the first `\|`), split at `,`. The third part, with white space trimmed at both ends, is kept when it exists and is not empty. The kept parts are joined with one U+0020 SPACE. |
| Korean | the empty string (Meguri rule, Q4) |

R1693 defines `reading` as the readings inside the brackets with the pitch suffix after `;` dropped for CJK text, and as the IPA part for European text. The CJK row keeps only the reading and drops the dictionary form, and joining several readings is a Meguri rule (Q1). The European row takes the IPA as the third comma-separated part of the first alternative. In the European form of S2 the third part is the gender, so this row may not match the add-on's output (Q2).

## S7. What never changes

- Stripping never removes a tag, never decodes or encodes an entity and never changes the case or the normalization form of any character. The note-add path normalizes stored fields later (`notes/note-records` N2).
- A value with no annotation, no raw group and, in CJK, no removable space is returned unchanged.
- Stripping an already stripped value gives the same value, except when the value still holds bracket text that S4 recognizes, such as a CJK `[x]` placed right after a character.

## S8. Examples

All texts are synthetic.

CJK stripping:

| Input | Stripped |
|---|---|
| `猫[ねこ;h]` | `猫` |
| `食[た;h]べる` | `食べる` |
| `猫[ねこ;h] が[;] 好[す;n2]き{。}` | `猫が好き。` |
| `<strong>食[た;h]べた</strong>{。}` | `<strong>食べた</strong>。` |
| `東京[とうきょう;h]{ }タワー[;a]` | `東京 タワー` (protected space) |
| `ノート[;n1] の[;] iPhone 版[ばん;h]` | `ノートの iPhone 版` |
| `猫 [ねこ]` | `猫 [ねこ]` (space before `[`; `[` is not a CJK character, so the space stays) |
| `[;h]猫` | `[;h]猫` (nothing before `[`) |
| `食[た;h` | `食[た;h` (no `]`) |
| `<b>漢字</b>[かんじ]` | `<b>漢字</b>[かんじ]` (the `[` starts a segment) |
| `本[ほん;h]{[1]}` | `本[1]` (protected) |
| `ね{、}` | `ね、` |
| `{。}[x]` | `。` (the `}` before `[` is a character of the segment, S4) |

European and Korean stripping:

| Input | Family | Stripped |
|---|---|---|
| `Hund[Hund,nn,m]` | European | `Hund` |
| `Der[der,art,m] Hahn[Hahn,nn,m] kräht[krähen,v,].` | European | `Der Hahn kräht.` |
| `streng[streng,adj,\|streng,adv,]` | European | `streng` |
| `(im Haus)[im Haus,adv,]` | European | `im Haus` |
| `Sie sägen)[sägen,v,]` | European | `Sie sägen)` (no `(`) |
| `<strong>perro[perro,n,m]</strong> feo[feo,adj,]` | European | `<strong>perro</strong> feo` |
| `¿Vas[ir,v,] a bañarte[bañarse,v,]?` | European | `¿Vas a bañarte?` |
| `Name[1] und Text [sic]` | European | `Name[1] und Text [sic]` |
| `{Hund}[Hund,nn,m]` | European | `{Hund}` (braces are text outside CJK) |
| `학교[학교,n,]에[에,adp,] 가요[가다,v,]` | Korean | `학교에 가요` |

Readings:

| `targetWord` | Family | `reading` |
|---|---|---|
| `猫[ねこ;h]` | CJK | `ねこ` |
| `見[み;h]る 書[か;k1]く` | CJK | `みか` |
| `ノート[;n1]` | CJK | (empty) |
| `行[い,行く;h]` | CJK | `い` |
| `食べる` | CJK | (empty) |
| `Hund[Hund,nn,m]` | European | `m` (see Q2) |
| `Haus[Haus,nn,n,haʊs]` | European | `n` (see Q2) |
| `streng[streng,adj,\|streng,adv,]` | European | (empty) |
| `학교[학교,n,]` | Korean | (empty) |

## S9. Tests

These are pure functions with no oracle. Meguri's unit tests (`core/crates/meguri-servers/tests/`, U139) take every row of S8 as a case and add a property test: stripping never changes a tag or a character outside annotations, parentheses, raw-group braces and removed space runs.

## S10. Open questions

- **Q1. CJK reading.** The add-on's reading is the bracket content without the `;pitch` part. Does it keep the dictionary form after the comma, and how does it combine the readings of several annotations and the text outside them? S6 keeps only the reading part and joins the parts with nothing between them. The add-on's output for `見[み;h]る 書[か;k1]く` and `行[い,行く;h]` would settle it.
- **Q2. European reading.** Does the add-on's IPA come from the third comma-separated part, which in S2's European form is the gender, or from a fourth part that S2's form does not show? S6 takes the third part. The add-on's output for a word that carries an IPA part would settle it.
- **Q3. CJK spaces.** Does the add-on remove the spaces between annotated Japanese tokens, and by which rule? S5 step 3 is Meguri's rule.
- **Q4. Korean.** What is the add-on's Korean rule? S4 to S6 treat Korean as European for stripping, with an empty reading. The default profiles of R1714 include no Korean profile.
- **Q5. European braces.** Raw groups are known only in Japanese text. S5 leaves braces in European text unchanged.
- **Q6. Parenthesized surfaces.** Can the surface of `(word)[…]` hold several words separated by spaces? S5 allows it, which also covers a single word.
