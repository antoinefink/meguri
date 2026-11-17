# Migaku routing profiles and language detection

Spec: servers/migaku-routing
Requirements: R1692, R1710, R1711, R1712, R1713, R1714, R1715, R1716, R1717, R1718, R1719, R1720, R1947; R1665 (local storage); AE71, AE87
Anki: 26.09.3
Scenarios: conformance/recordings/servers/migaku/routing/ (`servers/transcripts` T2: transcripts live under `recordings/`, not `scenarios/`). Written so far (declared, `servers/transcripts` T13): `no-profile`, `tie-recent-route`, `tie-60-minutes`, `tie-61-minutes`, `fallback-not-last-route`, `manual-move-recent`, `override` and `counts-only-on-commit`. The other D15.1 rows can be written with `servers/transcripts` T5.1 (`migaku_routing`: profiles, fallback, override), T4.1's `migaku_pending_switch`, T7.3's `routing` (D2.3's operations and `record_manual_move`), `received_log` and `lifecycle` steps and T7.1's `advance`, except the three that delete a deck or a note type while the scenario runs (`deck-deleted`, `notetype-deleted`, `pending-profile-deck-deleted`): T7.3's `collection` step has no deck or note type deletion, and T5.1 cannot start from a profile whose deck or note type is gone (D2.1 refuses to save it). Those three are U138 Rust tests only until that step admits the deletions. Received counts are reached only through commits (T5.1 starts the state empty), so `tie-default-fallback` and `two-japanese-profiles` commit their cards under the override first. The server transcript format is `servers/transcripts`, whose runner (U136) does not exist yet. Until U136 lands, U138 verifies D15.1's rows through its own Rust tests (`tests/migaku_routing.rs`, `migaku_detect.rs`, `migaku_profiles.rs`). The synthetic corpus `conformance/corpora/migaku-routing-synthetic.jsonl` exists and holds exactly the lines of D15.2; a change to either changes both.

How Meguri chooses the note type, deck, field map, default values and tags for a card that the Migaku browser extension (1.30 and 1.31) sends to `POST /receive`. The payload names none of them (R1692). The Migaku Anki add-on 0.6.6 has no automatic routing: it uses one configured note type and deck, or the open Add window's. Everything here except the field sources of D3 and the field layout of D4's example is Meguri's own design, written to reproduce the user's history (R1718, R1947). Anki has no part in it.

The protocol around `/receive` is `servers/migaku-protocol`; the receive pipeline that calls this file's resolution (D13), maps fields, stores media and adds the note is `servers/migaku-receive` (U139); bracket-syntax stripping and readings are `servers/migaku-syntax`; Recently received, moves and `/create` are `servers/migaku-received-log` (U140).

## D1. Terms

- **Profile**: a destination profile (D2). Only profiles the user confirmed exist; a draft (D6) is not a profile.
- **Payload**: the JSON body of `/receive` as `servers/migaku-receive` parses it; a missing key is the empty string or the empty list.
- **Routing clock**: the time a request arrived (its arrival, `servers/migaku-receive` P1), in milliseconds since the epoch, on the host's wall clock (`servers/host-security` H21). The corpus replay injects it (D14.3).
- **Language**: one code of the table below. The English name is what Meguri shows and what D6 matches in note type names.

  | Code | English name | Detection (D9, D10) |
  |---|---|---|
  | `ja` | Japanese | script |
  | `zh` | Chinese | script (Han) |
  | `yue` | Cantonese | script (Han) |
  | `ko` | Korean | script |
  | `de` | German | Latin scoring |
  | `es` | Spanish | Latin scoring |
  | `fr` | French | never detected |
  | `pt` | Portuguese | never detected |
  | `it` | ISO 639-1's English name for `it` | never detected |
  | `vi` | Vietnamese | never detected |
  | `en` | English | never detected |

  A profile of a "never detected" language receives cards only through the uncertain route of D11, the override (D11) or manual moves.
- **Usable deck**: `notes/deck-placement-and-position#D1`.
- **unicase**: `text/unicode-primitives#U2`.

## D2. Profiles (R1710, R1665)

### D2.1 What a profile holds

| Value | Type | Default | Notes |
|---|---|---|---|
| id | positive integer | allocated | increases with each new profile; never reused; orders ties (D11) |
| name | text | (D6 or the user's) | non-empty after trimming white space; unique among profiles under unicase |
| language | a D1 code | (D6 or the user's) | |
| note type | note type id | | stored by id, so a rename keeps the profile working |
| deck | deck id | | stored by id; must be a usable deck when saved |
| field map | per field: field id (when the field has one), field name, source (D3) | inferred (D4) | |
| defaults | per field: field id (when the field has one), field name, default text | none (D5, D6) | |
| tags to add | list of tags | empty | canonicalized by the note-add path (`tags/tags`) |
| normalize audio | on or off, target loudness in LUFS | off, −20 | a number from −40 to −10 inclusive; applied by `servers/migaku-receive` (R1697) |

- Profiles, the routing settings (fallback profile, override) and the routing state (D12) live in the integrations store (`servers/host-security` H10.1, in tables that U138's migration adds), never in the collection. Sync and `.apkg` export never carry them (R1665).
- Saving a profile runs these checks in this order and stops at the first that fails. It then returns that error with the failing value and saves nothing:

  | Error | Fails when |
  |---|---|
  | `name_empty` | the name is empty after trimming white space |
  | `name_duplicate` | another profile has the same name under unicase |
  | `language_unset` | no language is set |
  | `notetype_missing` | the note type id does not exist |
  | `deck_unusable` | the deck id does not exist or is not a usable deck |
  | `field_unknown` | a field map or default entry matches no field of the note type (D2.2) |
  | `loudness_out_of_range` | the target loudness is outside −40 to −10 |

- Editing a profile keeps its id, its received count and the last route (D12) if it names it, whatever value changed, the language and the note type included.
- Deleting a profile removes it, its routing counter, the override if it named it, the fallback setting if it named it, and the last route (D12) if it named it.

### D2.2 Field matching

A field map or default entry refers to a field of the profile's note type by the field's id when both the entry and the field have one (`notetypes/records` N2, R905); otherwise by exact name; otherwise by name under unicase. An entry and a field that both have ids match only when the ids are equal; name matching applies only to a pair where the entry or the field has no id. A field of the note type that no entry matches has source `none` and no default. An entry that matches no field is ignored and reported by D7.3.

### D2.3 Interface for the app (U144, U145)

These operations reach Swift through the FFI (`core/crates/meguri-ffi/src/servers.rs`). Each runs on the integrations store and the collection as they are at the call.

| Operation | Input | Result |
|---|---|---|
| list profiles | none | every profile in id order, with its D2.1 values and its D7.3 warnings |
| save profile | a profile; no id for a new one | the stored profile, or one D2.1 save error |
| delete profile | a profile id | nothing; an unknown id changes nothing |
| infer field map | a note type id | D4's sources for its fields, in ordinal order; `notetype_missing` for an unknown id |
| draft profiles | none | D6's drafts, in D6's order |
| confirm drafts | the drafts the user kept, possibly edited | the stored profiles; or the first D2.1 save error, with the draft it belongs to, and nothing stored |
| set fallback profile | a profile id or none | nothing; an unknown id is refused as `profile_missing` |
| set override | a profile id or Automatic (none) | nothing; an unknown id is refused as `profile_missing` |
| routing settings | none | the fallback profile and the override |

Confirming drafts checks them all before storing any, then stores them in the order given.

## D3. Field sources (R1693, R1711)

Each field of the destination note type has exactly one source:

| Source | Value given to the field (`servers/migaku-receive` builds it) |
|---|---|
| `targetWord` | the payload's `targetWord` |
| `sentence` | the payload's `sentence` |
| `translation` | the payload's `translation` |
| `definitions` | the payload's `definitions` |
| `exampleSentences` | the payload's `exampleSentences` |
| `notes` | the payload's `notes` |
| `sentenceAudio` | one sound reference per `sentenceAudio` item, joined with `"\n<br>\n"` |
| `wordAudio` | the same for `wordAudio` |
| `images` | every `images` item as an image reference |
| `firstImage` | the first `images` item |
| `restImages` | the second `images` item onward |
| `targetWordNoSyntax` | `targetWord` with Migaku bracket syntax removed (`servers/migaku-syntax`) |
| `sentenceNoSyntax` | `sentence` with Migaku bracket syntax removed |
| `reading` | the reading of `targetWord` (`servers/migaku-syntax`) |
| `none` | the empty string |
| `default_only` | the empty string |

- `none` and `default_only` give the same field value; after D5, both fields hold their default. `default_only` exists so the profile editor (U145) can show a field that is filled only by its default. D4 never produces it.
- A source may be given to several fields by the user. D4 gives each source to at most one field.
- A profile **maps a field** when at least one field has a source other than `none` and `default_only`. A profile that maps no field cannot receive (D7.2).

## D4. Field-map inference (R1711, R1712)

Meguri infers a field map from the note type's field names when D6 drafts a profile and when the user picks a note type in the profile editor.

1. **Normalize** each field name: NFC (`text/unicode-primitives#U3`), Unicode lower case, every `_` and `-` replaced by a space, runs of white space collapsed to one space, white space removed at both ends.
2. **Match** each normalized name against the rules in this order; the first rule that matches gives the source. "Has `x`" means the normalized name contains `x` as a substring.

   | Order | Rule | Source |
   |---|---|---|
   | 1 | starts with `is ` | `none` |
   | 2 | has `sentence` and has `audio` | `sentenceAudio` |
   | 3 | has `audio` and (has `word` or has `vocab`) | `wordAudio` |
   | 4 | has `screenshot` | `firstImage` |
   | 5 | has `image` or has `picture` | `restImages` when another field of the note type has rule 4 as its first matching rule, otherwise `images` |
   | 6 | has `example` | `exampleSentences` |
   | 7 | has `translation` | `translation` |
   | 8 | has `definition` | `definitions` |
   | 9 | has `sentence` | `sentence` |
   | 10 | has `word` or has `vocab` | `targetWord` |
   | 11 | is `note` or `notes` | `notes` |
   | 12 | has `reading` | `reading` |
   | — | anything else | `none` |

3. **One field per source**: when several fields got the same source (other than `none`), the field with the lowest ordinal keeps it and the others get `none`.

The rules use English words only; field names in other languages get `none` (D17 item 1).

The user's 11-field Migaku Custom note types infer the R1712 map:

| Field | Normalized | Rule | Source |
|---|---|---|---|
| Sentence | `sentence` | 9 | `sentence` |
| Translation | `translation` | 7 | `translation` |
| Target Word | `target word` | 10 | `targetWord` |
| Definitions | `definitions` | 8 | `definitions` |
| Screenshot | `screenshot` | 4 | `firstImage` |
| Sentence Audio | `sentence audio` | 2 | `sentenceAudio` |
| Word Audio | `word audio` | 3 | `wordAudio` |
| Images | `images` | 5 (Screenshot matches rule 4) | `restImages` |
| Example Sentences | `example sentences` | 6 | `exampleSentences` |
| Is Vocabulary Card | `is vocabulary card` | 1 | `none` |
| Is Audio Card | `is audio card` | 1 | `none` |

More inference cases:

| Field names (in order) | Inferred sources |
|---|---|
| `Front`, `Back` | `none`, `none` (the profile maps no field) |
| `Word`, `Image`, `Audio` | `targetWord`, `images`, `none` |
| `Vocab_Audio`, `Picture`, `Screenshot` | `wordAudio`, `restImages`, `firstImage` |
| `Sentence`, `Sentence (copy)` | `sentence`, `none` (rule 3 of the steps) |
| `Notes`, `My notes` | `notes`, `none` |
| `Reading`, `Word Reading` | `reading`, `targetWord` |

## D5. Default values (R1713)

- After the field values are built (D3), every field whose value is exactly the empty string and that has a non-empty default gets the default. A field with a non-empty value keeps it; no trimming or emptiness test beyond the empty string applies.
- Defaults are plain text written as given, like any field content.
- `servers/migaku-receive` applies this step; `servers/migaku-received-log` reuses it for `/create` (D13.2) and for moves.

The defaults that D6 proposes, by language, for fields named (under unicase) as below. They reproduce the user's own add-on hook that filled these flags when both were blank:

| Language | `Is Vocabulary Card` | `Is Audio Card` |
|---|---|---|
| `de`, `es` | `x` | `x` |
| `ja` | (none) | `x` |
| any other | (none) | (none) |

## D6. First enable: draft profiles (R1714)

When the Migaku integration is on and the integrations store holds no profile, Meguri computes a list of draft profiles for the user to confirm (U145). Nothing is stored until the user confirms; confirming stores the drafts the user kept, as profiles, in the list's order.

1. **Migaku note types**: every note type whose name contains `migaku` (Unicode lower case comparison) and that has at least one note. Note types with no note (the add-on's stock types) get no draft.
2. **Language**: the first language of D1's table, in table order, whose English name appears in the note type's name as a whole word (letters on neither side), compared under unicase. With none, the draft's language is unset and the user must choose one before confirming it.
3. **Deck**: count the note type's notes whose id (its creation time in milliseconds) is within the 90 days (7,776,000,000 ms) before now, where now comes from the same injectable clock as the routing clock (D1); if there is none, count all its notes. Each counted note belongs to the deck of its card with the lowest template ordinal, or to that card's original deck when the card is in a filtered deck (`decks/decks#D9`). The deck with the most counted notes wins; a tie goes to the deck holding the newest counted note, then to the lower deck id. A winner that is not usable is skipped for the next one; with none usable, the draft has no deck and the user must choose one.
4. **Name**: the language's English name. When several drafts share a language, each of them is named `<English name> (<note type name>)`. A draft with no language is named after its note type.
5. **Field map**: D4 on the note type's fields. **Defaults**: D5's table for the draft's language. **Tags to add**: none. **Normalize audio**: off.
6. **Order**: drafts are listed by language in D1's table order, then by note type name under unicase.

For the user's collection this gives three drafts (the three stock Migaku note types have no notes):

| Name | Language | Note type | Deck | Defaults |
|---|---|---|---|---|
| Japanese | `ja` | Migaku Japanese Custom | 🇯🇵 Japanese::🔊 Audio::4. Mining | Is Audio Card `x` |
| German | `de` | Migaku German Custom | 🇩🇪 German::Mining | Is Vocabulary Card `x`, Is Audio Card `x` |
| Spanish | `es` | Migaku Spanish Custom | 🇪🇸 Spanish::Mining | Is Vocabulary Card `x`, Is Audio Card `x` |

Settings offers the same computation again on demand ("Suggest profiles"); its drafts never replace stored profiles without the user's confirmation.

## D7. Destination checks and errors (R1715, R1703)

### D7.1 Order of checks

Resolution (D13) fails, with HTTP 400 in the error shape of `servers/migaku-protocol` P8 (`servers/migaku-receive` writes the reply and the Failed row), in this order:

| Condition | Message |
|---|---|
| no profile exists | `No Migaku destination is set up in Meguri.` |
| the chosen profile's note type no longer exists | `Destination note type for <profile name> no longer exists.` |
| the chosen profile's deck no longer exists or is not usable | `Destination deck for <profile name> no longer exists.` |
| the chosen profile maps no field (D3) | `No fields to map to.` |

- `<profile name>` is the profile's name as stored. When both the note type and the deck are gone, the note type message wins.
- Meguri never replaces a failing profile with another one: not another profile of the same language, not the fallback profile (R1715).
- `No fields to map to.` is the string the extension already recognizes (R1703). An all-keys-missing payload on a profile that maps fields is not this error; its result is `servers/migaku-receive`'s.

### D7.2 When the checks run

- Resolution (D13) runs when `servers/migaku-receive` asks for the destination (its stage 3), with the profiles and the routing state as they are then; a request that the commit sends back to that stage (a collection replacement, a changed field list) is resolved again.
- The note type and deck checks run again at commit, on the generation the commit runs on (`servers/migaku-receive`). A pending entry keeps the profile and the plan chosen at its arrival and is never routed again. When it drains, exactly these checks run, in this order, and the first that fails turns it into a Failed row with its `servers/migaku-receive` P19 message (P15): the profile no longer exists; its note type or its deck no longer exists (the note type first, with D7.1's messages); the note type's field list differs from the one the plan was built from. The `No fields to map to.` check does not run again: the stored plan already passed it.

### D7.3 Profile warnings

The profile list that the UI reads (U145) carries, for each profile, the warnings that apply now:

| Warning | When |
|---|---|
| `notetype_missing` | the note type id no longer exists |
| `deck_missing` | the deck id no longer exists or is not usable |
| `no_mapped_field` | the profile maps no field |
| `stale_field_entry` | a field map or default entry matches no field of the note type (D2.2) |

Warnings are computed on every read of the profile list; they are not stored.

## D8. Detection input

Detection is a pure function of `targetWord` and `sentence`. It never reads the collection, the profiles' notes or the clock; the profile set enters only through D9's Han rule.

1. **Join**: `targetWord`, one space, `sentence`.
2. **Tags out**: every HTML tag (a `<` followed by an ASCII letter, `/` or `!`, up to and including the next `>`) is replaced by one space. A `<` with no later `>`, or followed by anything else, is ordinary text.
3. **References**: the character references `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`, `&apos;`, `&nbsp;` (to U+00A0), `&#<decimal>;` and `&#x<hex>;` (`x` or `X`, hex digits in either case) are decoded; any other `&` text is kept. A numeric reference whose value is 0, a surrogate (U+D800 to U+DFFF) or above U+10FFFF stays as text. Decoding is a single pass: `&amp;lt;` gives `&lt;`.
4. **NFC** (`text/unicode-primitives#U3`). The result is the **prepared text**.
5. **Bracket groups**: a `[`, then one or more characters other than `[` and `]`, then `]`. A `[` with no such closing `]` is ordinary text. A group's content is split on `|` into **alternatives**; each alternative is split on `,` into parts, each part trimmed of white space: part 1 is the **lemma**, part 2 the **tag**, part 3 the **gender**; missing parts are empty.
6. **Surface text**: the prepared text with every bracket group removed and every `{` and `}` removed (the characters between them stay).
7. **Tokens**: the maximal runs of letters (Unicode categories L and M) of the surface text, each in Unicode lower case.

| Input (`targetWord` · `sentence`) | Prepared text | Alternatives (lemma, tag, gender) | Tokens |
|---|---|---|---|
| `Hund[Hund,nn,m]` · `<strong>Der[der,art,m]</strong> Hund.` | `Hund[Hund,nn,m]  Der[der,art,m]  Hund.` | (Hund, nn, m), (der, art, m) | hund, der, hund |
| `gato[gato,n,m\|gata,n,f]` · `¿Ves al gato?` | `gato[gato,n,m\|gata,n,f] ¿Ves al gato?` | (gato, n, m), (gata, n, f) | gato, ves, al, gato |
| `{、}` · `a &amp; b` | `{、} a & b` | none | a, b |

## D9. Script classification (R1716)

Classification looks at every character of the prepared text, bracket contents included, and takes the first row that applies:

| Order | Text contains | Result |
|---|---|---|
| 1 | a kana character | `ja`, confident |
| 2 | a Hangul character | `ko`, confident |
| 3 | a Han character | D9.1 |
| 4 | a CJK symbol or full-width form | `ja`, confident |
| 5 | none of these | Latin scoring (D10) |

Character classes:

| Class | Code points |
|---|---|
| kana | U+3040–U+309F, U+30A0–U+30FF, U+31F0–U+31FF, U+FF66–U+FF9F, U+1B000–U+1B16F |
| Hangul | U+1100–U+11FF, U+3130–U+318F, U+A960–U+A97F, U+AC00–U+D7AF, U+D7B0–U+D7FF, U+FFA0–U+FFDC |
| Han | U+3400–U+4DBF, U+4E00–U+9FFF, U+F900–U+FAFF, U+20000–U+2FA1F |
| CJK symbol or full-width form | U+3000–U+303F, U+FF00–U+FFEF |

Rows 1 and 2 keep R1716's order: kana means Japanese even with Hangul present; Hangul without kana means Korean even with Han or CJK symbols present.

### D9.1 Han without kana or Hangul

The result depends on the languages of the existing profiles:

| Profiles with `ja` | Profiles with `zh` or `yue` | Result |
|---|---|---|
| yes | no | `ja`, confident |
| yes | yes | uncertain |
| no | only `zh` | `zh`, confident |
| no | only `yue` | `yue`, confident |
| no | both `zh` and `yue` | uncertain |
| no | no | `ja`, confident (then no profile: D11 step 4) |

## D10. German against Spanish (R1717)

Text that reaches row 5 of D9 is scored for German (`de`) and Spanish (`es`). Each alternative's tag, gender and lemma are converted to Unicode lower case before every comparison below. Each signal adds its points to one language, once per occurrence:

| Signal | Counted on | German | Spanish | Points |
|---|---|---|---|---|
| exclusive tag | each alternative whose tag, in lower case, is in the list | `nn`, `nnp`, `prep`, `aux` | `n`, `propn`, `adp` | 2 |
| gender on adjective or verb | each alternative whose tag is `adj` or `v` and whose gender is `m` or `f` (lower case) | — | yes | 1 |
| verb lemma ending | each alternative whose tag is `v`, `verb` or `aux` and whose lemma, in lower case, ends with a listed ending (one point per alternative, even when more than one listed ending matches) | `en`, `eln`, `ern` | `ar`, `ir`, `arse`, `erse`, `irse` | 1 |
| letters | each occurrence in the surface text | `ä`, `ö`, `ü`, `ß`, `Ä`, `Ö`, `Ü`, `ẞ` | `ñ`, `Ñ`, `¿`, `¡`, `á`, `é`, `í`, `ó`, `ú`, `Á`, `É`, `Í`, `Ó`, `Ú` | 1 |
| stopwords | each token equal to a listed word | D10.1 | D10.1 | 1 |

**Decision**: with German score g and Spanish score s, g − s ≥ 1 gives `de`, confident; s − g ≥ 1 gives `es`, confident; g = s (including 0 = 0) is uncertain.

### D10.1 Stopword lists

German: `der`, `die`, `und`, `ich`, `ist`, `war`, `mit`, `auf`, `für`, `ein`, `dem`, `wir`, `sie`, `zu`, `von`, `im`, `wie`, `was`, `bei`, `nur`, `oder`, `hat`, `bin`, `uns`, `ihr`, `ihm`, `ihn`, `zum`, `zur`, `vom`, `über`.

Spanish: `el`, `la`, `los`, `las`, `y`, `que`, `de`, `en`, `un`, `una`, `por`, `para`, `con`, `no`, `lo`, `se`, `del`, `al`, `más`, `muy`, `son`, `su`, `sus`, `le`, `les`, `me`, `mi`, `ya`, `hay`, `eso`, `sin`, `tan`, `nos`, `ese`, `esa`, `fue`, `todo`.

Words that are common in both languages (`es`, `das`, `des`, `den`, `a`) are in neither list. The lists hold only words that the private-token guard (R1940) lets a committed file hold; longer common words that the user's collection contains are left out until the product owner decides otherwise (D17.11). A change to the lists, the lemma suffixes or the points is a change to this spec, checked again by U249's private run (R1718).

### D10.2 Examples

| `targetWord` | `sentence` | German | Spanish | Result |
|---|---|---|---|---|
| `Hahn[Hahn,nn,m]` | `Der[der,art,m] Hahn[Hahn,nn,m] kräht[krähen,v,].` | 7 (nn 2 + nn 2, `krähen` 1, `der` 1, `ä` 1) | 0 | `de` |
| `perro[perro,n,m]` | `El[el,det,m] perro[perro,n,m] ladra[ladrar,v,].` | 0 | 6 (n 2 + n 2, `ladrar` 1, `el` 1) | `es` |
| `pálida[pálido,adj,f]` | `La[el,det,f] gata[gato,n,f] es[ser,v,] pálida[pálido,adj,f].` | 0 | 7 (adj f 1 + 1, n 2, `la` 1, `á` 2) | `es` |
| `sägen[sägen,v,]` | `Sie sägen.` | 4 (`sägen` 1, `ä` 2, `sie` 1) | 0 | `de` |
| `bañarse[bañarse,v,]` | `¿Vas a bañarte?` | 0 | 4 (`bañarse` 1, `ñ` 2, `¿` 1) | `es` |
| `Café` | `Ist das Café zu?` | 2 (`ist` 1, `zu` 1; `das` is in neither list) | 2 (`é` 2) | uncertain |
| `Haus` | `Das Café ist zu.` | 2 (`ist` 1, `zu` 1) | 1 (`é` 1) | `de` (margin 1) |
| `Peña` | `Peña und Kühn` | 2 (`und` 1, `ü` 1) | 2 (`ñ` 2) | uncertain |
| `streng[streng,adj,\|streng,adv,]` | `Streng vertraulich.` | 0 | 0 | uncertain (AE71) |
| `Berlin[Berlin,nnp,]` | `in[in,prep,\|in,adp,] Berlin` | 4 (nnp 2, prep 2) | 2 (adp 2) | `de` |
| `glurbt` | `Es glurbt[glurben,aux,].` | 3 (aux 2, `glurben` 1: the tag `aux` also counts the verb ending) | 0 (`es` is in neither list) | `de` |
| `Madrid[Madrid,propn,]` | `en Madrid` | 0 | 3 (propn 2, `en` 1) | `es` |
| `wandern[wandern,v,]` | `Wir zorbeln[zorbeln,v,] zur Straße.` | 5 (`wandern` 1 by `ern`, `zorbeln` 1 by `eln`, `ß` 1, `wir` 1, `zur` 1) | 0 | `de` |
| `plim` | `¡Plim ya!` | 0 | 2 (`¡` 1, `ya` 1) | `es` |
| `blicado` | `Ya blicado[blicar,v,m].` | 0 | 3 (v m 1, `blicar` 1, `ya` 1) | `es` |

## D11. Choosing a profile (R1719, R1720)

For each `/receive` payload, at the routing clock `at`, with the profiles and the routing state as they are when resolution runs (D7.2):

1. **No profile**: D7.1's first error.
2. **Override**: when the override names a profile, that profile, decision `Override`, route `override`. Detection still runs and its result is recorded (D13), but it does not change the choice.
3. **Confident language with a profile**: when detection (D9, D10) is confident for language L and at least one profile has language L, the profile of language L with the highest received count (D12); a tie goes to the lowest profile id. Decision `Auto`, route `detected`.
4. **Otherwise** (uncertain, or confident for a language that no profile has). A confident detection of a language that no profile has is treated as uncertain from here on: for routing, for the confidence that D13 reports and for the received log.
   - when the last route (D12) exists and 0 ≤ `at` − its time ≤ 3,600,000 ms (60 minutes, inclusive), its profile; decision `Fallback`, route `recent`;
   - otherwise the fallback profile: the profile the user chose as fallback, or, with no choice set, the profile with the highest received count (tie: lowest profile id); decision `Fallback`, route `fallback_profile`.
5. **Checks**: the chosen profile goes through D7.1's checks. A failure is the request's result; no other profile is tried.

The override (R1720) is a stored setting, "Automatic" (none) or one profile id, changed from Recently received or the Mac menu-bar command (U145). It stays until the user sets it back to Automatic or deletes its profile. While it is set, Settings shows it (U144).

The decision kinds that routing produces are `Auto`, `Fallback` and `Override`. `Manual` is set on a received-log row by a move (`servers/migaku-received-log`). Every card that step 4 routed is `Fallback` and is reported as uncertain (D13.1), whether detection itself was uncertain or found a language with no profile. That is what the Recently received badge counts (R1723), and every `Fallback` card is an uncertain card, as `servers/migaku-received-log` L4 says.

## D12. Routing state

Stored in the integrations store and kept across restarts:

| Value | Changed by |
|---|---|
| **received count** per profile | +1 when a `/receive` note is committed to that profile, whatever the decision. Not changed by `/create`, by moves, by failures or by retries answered with an existing note. |
| **last route**: a profile id and a time | set to (profile, `at`) when a `/receive` note with decision `Auto` is committed, and to (profile, time) by a manual move: `record_manual_move(profile, at)`, which U140 calls for each successful move, undo and redo. A new value replaces the stored one only when its time is not earlier than the stored time. |
| **fallback profile** | the user's choice, or none ("the profile with the most received cards") |
| **override** | D11 |

- `Fallback` and `Override` commits never change the last route.
- A pending entry (`servers/migaku-receive`) was routed at its arrival. Its commit, when it drains, updates the state with its arrival time as `at`, so a later card's earlier commit is never overwritten (the "not earlier" rule above).
- Requests are resolved in arrival order, but a request can be resolved before an earlier one has committed. Its route then does not see that earlier commit's update. The corpus replay (D14.3) posts one payload at a time and waits for each reply, so its results do not depend on this.
- Deleting a profile clears every value that names it (D2.1).
- The corpus replay (D14.3) starts from an empty state: no counts, no last route, no override, and the fallback profile the run names.

## D13. The resolution result

### D13.1 For `/receive`

Resolution takes the payload and `at` and returns either a D7.1 failure or a result. A failure at D11 step 1 (no profile) carries only the message: no profile, decision, route or detection, so its Failed row has none (`servers/migaku-received-log` L2, L4). A failure at D11 step 5 carries the message and also the chosen profile's id and name, the decision, the route and the detection, which `servers/migaku-receive` P20 writes on the Failed row. A result holds:

| Member | Content |
|---|---|
| profile | id and name |
| note type id, deck id | the profile's |
| fields | for each field of the note type in ordinal order: ordinal, name, source (D3, after D2.2), default (D5; empty when none) |
| tags to add | the profile's list |
| normalize audio | on or off, and the target loudness |
| decision | `Auto`, `Fallback` or `Override` |
| route | `detected`, `recent`, `fallback_profile` or `override` |
| detection | language code or none, `confident` or `uncertain`, and the signals |

The language is the one D9 or D10 found, none when they found none. The confidence is `uncertain` when D9 or D10 was uncertain, and also when they were confident for a language that no profile has (D11 step 4). For example, Hangul with no Korean profile gives language `ko`, `uncertain`. With the override set, the confidence is what steps 3 and 4 would have given without it.

The signals are a list of `{signal, language, points}`. They are for display and debugging, but tests compare them, so their content and order are fixed:

- **Script** (D9 rows 1 to 4): exactly one entry, for the row that applied: `script:kana`, `script:hangul`, `script:han` or `script:cjk_symbol`, with that row's language (for `script:han`, D9.1's language, or none when D9.1 is uncertain) and 0 points. It is the whole list: no Latin signal follows it.
- **Latin** (D9 row 5): one entry per counted occurrence of D10, with D10's language and points: `tag:<tag>` and `ending:<ending>` (both in lower case; at most one listed ending can match a lemma, since no listed ending is the end of another), `gender:<m or f>`, `letter:<character>` (the character as it appears) and `stopword:<word>`. No points: no entry.
- **Order** of the Latin entries: by position in the prepared text (D8 step 4). An alternative's entries take the position of its bracket group's `[`, in the order tag, gender, ending, and the alternatives of one group follow each other in group order. A letter takes its own position; a stopword the position of its token's first character. At one position a letter comes before a stopword.

For example, D10.2's first row gives `tag:nn` (de, 2) for the target word's group, then for the sentence `stopword:der` (de, 1), `tag:nn` (de, 2), `letter:ä` (de, 1) and `ending:en` (de, 1) for `kräht[krähen,v,]`, whose `ä` comes before its group's `[`.

Resolution does not change the routing state; `servers/migaku-receive` reports the commit, and D12 applies then.

### D13.2 For legacy `/create`

`/create` names its note type and deck itself and is not routed (R1732). Its defaults come from the matching profile: the profile with the same note type and the same deck; else the profile with the lowest id among those with the same note type; else none (no defaults). `/create` never changes the routing state.

## D14. The routing corpus (R1718, R1947)

The tool is `meguri-cli migaku-corpus` (U138). The private runs over the user's collection are U249's, through U15's vault and output guard; U138 itself runs only the committed synthetic corpus (D15.2).

### D14.1 Which notes are received notes

- `extract --collection <path>` takes the notes of the note types given with `--notetype <id>` (repeatable; default: every Migaku note type of D6 step 1 whose language is `ja`, `de` or `es`) and, with `--since <ms>`, only notes whose id is at or after that time.
- R1718's set: the three Migaku Custom note types, no `--since`: 4,110 notes.
- R1947's and AE87's set, the notes the add-on created through `/receive`: the same note types, with `--since` set to the creation time of the first note that the add-on's log records as created through `/receive`. That value is private: U249's run configuration supplies it from the vault side, and no spec or committed file holds it. The run must find 483 German, 536 Spanish and 241 Japanese notes (1,260). Other counts are a stop condition the run reports; the run does not adjust the cut-off to reach them.
- `extract` writes, to the file given with `--out <path>`, the corpus format of D15.2: the setup line, built from D6's drafts for the selected note types, unchanged, with the fallback profile named by `--fallback <profile name>` (default: the first draft), then one line per note in note id order with `at` set to the note's id.

### D14.2 Rebuilding payloads

For each note, in note id order, using the field map that D4 infers for its note type. The note's **logical id** is its 1-based position in that order; the report names notes only by it.

- A field with source `targetWord`, `sentence`, `translation`, `definitions`, `exampleSentences` or `notes` gives that payload key its content unchanged.
- A field with source `sentenceAudio` or `wordAudio` gives one item per `[sound:<name>]` in the field, in order: `{"id": "<logical id>-<base>", "title": "", "input": "data:<type>;base64,<content>", "r2Url": null, "uploadId": null}`.
- The fields with sources `firstImage` and `restImages` give `images`: the `<img src="<name>">` references of the `firstImage` field, then those of the `restImages` field, each `{"id": "<logical id>-<base>", "title": "", "src": "data:<type>;base64,<content>", "alt": ""}`.
- `<base>` is `<name>` without its last extension. Prefixing the logical id keeps every item id distinct across notes, so no rebuilt payload matches an earlier one by media under `servers/migaku-receive` P12.
- `<type>` and `<content>` come from the name's last extension, in ASCII lower case, so that `servers/migaku-receive` P9 always finds a file extension:

  | Extension | `<type>` | `<content>` |
  |---|---|---|
  | `mp3`, `m4a`, `ogg`, `webm`, `wav`, `flac`, `aac` | `audio/mpeg`, `audio/mp4`, `audio/ogg`, `audio/webm`, `audio/wav`, `audio/flac`, `audio/aac` | base64 of the UTF-8 bytes of `<name>` |
  | `webp`, `jpg`, `jpeg`, `png`, `gif`, `avif`, `heic`, `heif`, `svg`, `bmp` | `image/webp`, `image/jpeg` (both), `image/png`, `image/gif`, `image/avif`, `image/heic` (both), `image/svg+xml`, `image/bmp` | the same |
  | any other of 1 to 10 characters `a`–`z` and `0`–`9` | `audio/<extension>` for an audio item, `image/<extension>` for an image item | the same |
  | none, or anything else | `application/octet-stream` | base64 of the bytes `ID3` followed by the UTF-8 bytes of `<name>` for an audio item; of the PNG signature (89 50 4E 47 0D 0A 1A 0A) followed by them for an image item |

- Every other key is absent. In particular the payload has no value for the fields with source `none`, such as Is Vocabulary Card and Is Audio Card, so the created note receives the profile's defaults (D5) there.
- The **expected destination** of the payload is: the D6 draft of the note's note type (its profile name and its language), the note type, the deck of the note's lowest-ordinal card (its original deck when the card is in a filtered deck), and the expected profile's defaults.

### D14.3 Replay and report

`replay --corpus <path> (--collection <path> | --scratch) [--report <path>]`:

- With `--collection`, replay works on a scratch copy of that collection, never on the file itself. Every note type and deck that the setup line names must exist there by name; a missing one stops the run before any request.
- With `--scratch`, replay starts from a new empty collection. For each setup profile it creates, when absent, the note type with the 11 fields of D4's example table in that order and one template, front `{{Sentence}}`, back `{{Translation}}`, and the deck by full name with the default deck options. The synthetic corpus (D15.2) runs this way.
- It creates the setup line's profiles in their order (ids 1, 2, …), each with D4's inferred field map for its note type and the defaults the line gives, else D5's table for its language. It sets the line's fallback profile, no override, and an empty routing state (D12).
- It runs a Migaku server in process, built with the host's test construction (`servers/host-security` H22: a test clock, the pass-through transcoder, the switch off) and served without a socket (H22 socketless serving). For each line after the setup, in file order, the clock gives `at` as the request's arrival time, so the same value is the routing clock (D1) and the arrival time that `servers/migaku-receive` uses, its 600 s retry window included. It posts the payload to `/receive` and waits for the reply before the next line.
- The run sees no real time, so a payload with no item whose `targetWord` and `sentence` equal those of a note committed from a line whose `at` is at most 600 s earlier is answered as a retry (`servers/migaku-receive` P12), as it would be live. The report counts it as `retry_matched`.

Each payload falls in exactly one bucket, keyed by the expected profile's language:

| Bucket | The reply and the created note |
|---|---|
| `correct` | a note was created; detection confident (D13.1); the note's profile has the expected language; its note type, deck and defaults are the expected destination's |
| `uncertain_correct` | the same, with detection uncertain |
| `wrong_destination` | a note was created in a profile of the expected language, but its note type, deck or a defaulted field differs from the expected destination |
| `wrong_confident` | a note was created in a profile of another language; detection confident |
| `uncertain_wrong` | the same, with detection uncertain |
| `retry_matched` | the reply named an existing note (P12) |
| `failed` | an error reply |

A defaulted field is a field with source `none` or `default_only` whose expected value is a default. The report also gives, per language, `uncertain`: the number of payloads whose detection was uncertain, whatever their bucket. For the synthetic corpus, each `expect` member that the line gives is also compared; a line whose bucket is right but whose `decision` or `detection` differs is listed as a miss with bucket `expect_mismatch`.

The report is one JSON object, written to `--report` (default: standard output). Languages with no payload are left out. For example, three payloads, the third failing:

```json
{"notes":3,"languages":{"de":{"correct":1,"uncertain_correct":1,"wrong_destination":0,"wrong_confident":0,"uncertain_wrong":0,"retry_matched":0,"failed":0,"uncertain":1},"es":{"correct":0,"uncertain_correct":0,"wrong_destination":0,"wrong_confident":0,"uncertain_wrong":0,"retry_matched":0,"failed":1,"uncertain":0}},"misses":[{"note":3,"bucket":"failed"}],"gates":{"R1718":false,"R1947":false}}
```

`misses` lists every payload not in `correct` or `uncertain_correct`, and every `expect_mismatch`, by logical id and bucket only; no field content, note type or deck name leaves the vault.

The gates:

- **R1718**: `wrong_confident`, `uncertain_wrong`, `retry_matched` and `failed` are 0 in every language, and the sum of `uncertain` is at most 0.5 % of the notes. `wrong_destination` does not fail this gate: R1718 is about the language, and older notes may sit in decks other than their draft's.
- **R1947 and AE87**: every payload is `correct` or `uncertain_correct`, so every other bucket is 0. This is also the gate of the synthetic corpus, together with no `expect_mismatch`.

## D15. Planned scenarios

### D15.1 Routing scenarios

Each row is one scenario of `conformance/recordings/servers/migaku/routing/`, id `servers/migaku/routing/<row>`, `source: declared`, in the `servers/transcripts` format. The header's Scenarios line says which are written and which cannot be written yet. A committed card in "Given" is a `/receive` of that kind sent at that time; a row's checks read the decision, route and detection from `received_log` rows. Unless a row says otherwise: profiles 1 German (`de`, Migaku German Custom, German::Mining), 2 Spanish (`es`, Migaku Spanish Custom, Spanish::Mining) and 3 Japanese (`ja`, Migaku Japanese Custom, Japanese::Mining), saved through D2.3 in that order (so ids 1, 2, 3) with the contents D6 would draft for them; a profile a row adds is saved after them and takes the next id. No override, no fallback choice, an empty routing state, and the clock at 10:00. Note types and decks are the scenario's own synthetic ones with the 11 fields of D4.

| Scenario | Given | Payload (`targetWord` · `sentence`) | Expected |
|---|---|---|---|
| `tie-recent-route` (AE71) | a German card committed with decision `Auto` at 09:50 | `streng[streng,adj,\|streng,adv,]` · `Streng vertraulich.` | profile 1, decision `Fallback`, route `recent`; detection uncertain |
| `tie-61-minutes` | a German card committed `Auto` at 08:59; fallback choice profile 2 | same | profile 2, `Fallback`, route `fallback_profile` |
| `tie-60-minutes` | a German card committed `Auto` at 09:00 | same | profile 1, `Fallback`, `recent` |
| `tie-default-fallback` | received counts 1: 3, 2: 5, 3: 5; no last route | same | profile 2 (highest count, lower id than 3), `Fallback`, `fallback_profile` |
| `fallback-not-last-route` | fallback choice profile 2; a German card committed `Auto` at 08:00; at 09:30 the uncertain payload below commits to profile 2 as `Fallback`, `fallback_profile` (the last route is 90 minutes old); at 09:45 the fallback choice set to profile 3 | same | profile 3, `Fallback`, `fallback_profile`: the 09:30 `Fallback` commit did not move the last route (had it moved it, the result would be profile 2, `recent`) |
| `manual-move-recent` | `record_manual_move(1, 10:00)`; clock 10:05 | same | profile 1, `Fallback`, `recent` |
| `german-tags` | — | `Hahn[Hahn,nn,m]` · `Der[der,art,m] Hahn[Hahn,nn,m] kräht[krähen,v,].` | profile 1, `Auto`, `detected`; signals include `tag:nn` twice |
| `spanish-tags` | — | `perro[perro,n,m]` · `El[el,det,m] perro[perro,n,m] ladra[ladrar,v,].` | profile 2, `Auto` |
| `scoring-table` | each D10.2 row runs from this starting state on its own | each row of D10.2 | the result of that row: a confident row goes to its language's profile, `Auto`; an uncertain row goes to profile 1 (no last route, all counts 0, lowest id), `Fallback`, `fallback_profile` |
| `kana-only` | — | `ねこ` · `ねこがいる。` | profile 3, `Auto` |
| `kanji-and-kana` | — | `猫[ねこ;h]` · `猫[ねこ;h]が好[す;a]き。` | profile 3, `Auto` |
| `fullwidth-only` | — | `！` · `ＡＢＣ` | profile 3, `Auto` |
| `hangul-no-profile` | a German card committed `Auto` at 09:30 | `고양이` · `고양이가 있어요.` | detection `ko` with no Korean profile, so reported `uncertain` (D13.1); profile 1, `Fallback`, `recent` |
| `han-ja-and-zh` | profile 4 Chinese (`zh`) added | `猫` · `我有猫` | detection uncertain; no last route and all counts 0, so profile 1 (lowest id), `Fallback`, `fallback_profile` |
| `han-ja-only` | — | `猫` · `我有猫` | profile 3, `Auto` |
| `han-zh-only` | profile 4 Chinese (`zh`) added, then profiles 1, 2 and 3 deleted | `猫` · `我有猫` | profile 4, `Auto`, detection `zh` confident |
| `override` | override = profile 2 | `Hund[Hund,nn,m]` · `Der Hund.` | profile 2, `Override`; detection `de` recorded; after the override is set back to Automatic, the same payload goes to profile 1 `Auto` |
| `override-no-last-route` | override = profile 2; a payload committed under it | then override off; `streng` · `Streng vertraulich.` | the `Override` commit left no last route but counted: profile 2 (count 1), `Fallback`, `fallback_profile` |
| `two-japanese-profiles` | profile 4 Japanese Reading (`ja`) added; received counts 3: 2, 4: 7 | `ねこ` · `ねこ。` | profile 4, `Auto` |
| `no-profile` | no profiles | `Hund` · `Der Hund.` | 400 `No Migaku destination is set up in Meguri.`; one Failed row with that error and no profile, decision, route or detection (D13.1); after a German profile is saved, Retry of that row creates the note from the kept payload |
| `deck-deleted` (U138 Rust test only) | profile 1's deck deleted | `Hund[Hund,nn,m]` · `Der Hund.` | 400 `Destination deck for German no longer exists.`; profile 1 shows `deck_missing`; profile 2 is not tried |
| `notetype-deleted` (U138 Rust test only) | profile 2's note type and deck deleted | `perro[perro,n,m]` · `El perro.` | 400 `Destination note type for Spanish no longer exists.` |
| `no-mapped-field` | profile 1's note type replaced by one with fields `Front`, `Back` (map inferred) | `Hund[Hund,nn,m]` · `Der Hund.` | 400 `No fields to map to.`; warning `no_mapped_field` |
| `counts-only-on-commit` | — | `perro[perro,n,m]` · `El perro.` with one `sentenceAudio` item whose `input` is `data:audio/mpeg;base64` (no comma: invalid, `servers/migaku-receive` P8), routed to profile 2 `Auto` and failing with `Could not read sentenceAudio item 1: invalid data URL.`; one minute later `streng[streng,adj,\|streng,adv,]` · `Streng vertraulich.` | the failure changed neither the count nor the last route: the second card goes to profile 1 (all counts 0, lowest id), `Fallback`, `fallback_profile`; a counted failure would give profile 2 |
| `pending-uses-arrival-time` | a German card committed `Auto` at 09:30; collection busy from 10:00 to 10:40 with the pending reply accepted (`servers/migaku-receive`); uncertain payload arrives at 10:20 and is queued | `streng[streng,adj,\|streng,adv,]` · `Streng vertraulich.` | routed at 10:20 to profile 1, `recent` (50 minutes after 09:30); committed at 10:40 to profile 1 |
| `pending-profile-deck-deleted` (U138 Rust test only) | as above with a confident German payload queued; profile 1's deck deleted before 10:40 | `Hund[Hund,nn,m]` · `Der Hund.` | at drain: Failed row `Destination deck for German no longer exists.`; no other profile tried |
| `field-map-custom` | — | (inference only) on the 11 fields | D4's R1712 table |
| `prefill` | a collection with Migaku German Custom (notes in German::Mining), Migaku Spanish Custom (Spanish::Mining), Migaku Japanese Custom (Japanese::Audio::Mining), and Migaku Japanese with no notes | (first enable) | three drafts as D6's table, with D5's defaults; the empty stock type gets none; nothing stored before confirmation |
| `prefill-deck-most-notes` | Migaku German Custom with 5 recent notes in German::Mining, 2 in German::Old, 9 older than 90 days in German::Old | (first enable) | deck German::Mining |
| `prefill-filtered` | the lowest-ordinal cards of the recent notes sit in a filtered deck whose original deck is German::Mining | (first enable) | deck German::Mining |
| `create-defaults` | `/create` with profile 1's note type and deck | (`servers/migaku-received-log`) | D5 defaults of profile 1 applied; routing state unchanged |

### D15.2 The synthetic corpus

`conformance/corpora/migaku-routing-synthetic.jsonl` holds one JSON object per line, in the format that `extract` also writes (D14.1).

- The first line is the setup: `{"profiles":[{"name":…,"language":…,"notetype":…,"deck":…,"defaults":{<field name>:<text>,…}},…],"fallback":<profile name>}`. Note types and decks are named; `defaults` may be left out, and then D5's table for the language applies.
- Every other line is `{"at":<ms>,"payload":{…},"expect":{…}}`, in increasing `at`. `expect` holds `profile`, `notetype` and `deck` (both kinds of corpus), and, in the synthetic corpus only, `decision`, `route` and `detection` (`{"language":<code or null>,"confidence":"confident"|"uncertain"}`).

The synthetic corpus, replayed with `--scratch` (D14.3):

- Setup: profiles `German` (`de`, Migaku German Custom, German::Mining), `Spanish` (`es`, Migaku Spanish Custom, Spanish::Mining) and `Japanese` (`ja`, Migaku Japanese Custom, Japanese::Mining), in that order, no `defaults` (D5's apply); fallback `Spanish`.
- Every payload has the given `targetWord` and `sentence`, no other text key, one `sentenceAudio` item `{"id":"s<line>-audio","title":"","input":"data:audio/mpeg;base64,SUQz","r2Url":null,"uploadId":null}` and one `images` item `{"id":"s<line>-shot","title":"","src":"data:image/png;base64,iVBORw0KGgo=","alt":""}`, where `<line>` is the line's number counted from 1 after the setup. The item ids differ on every line, so no line is a retry of another.
- `notetype` and `deck` in `expect` are those of the expected profile.

| Line | `at` | `targetWord` · `sentence` | Profile | Decision, route | Detection |
|---|---|---|---|---|---|
| 1 | 1767261600000 | `Hahn[Hahn,nn,m]` · `Der[der,art,m] Hahn[Hahn,nn,m] kräht[krähen,v,].` | German | `Auto`, `detected` | `de` confident |
| 2 | 1767261660000 | `perro[perro,n,m]` · `El[el,det,m] perro[perro,n,m] ladra[ladrar,v,].` | Spanish | `Auto`, `detected` | `es` confident |
| 3 | 1767261720000 | `ねこ` · `ねこがいる。` | Japanese | `Auto`, `detected` | `ja` confident |
| 4 | 1767261780000 | `猫[ねこ;h]` · `猫[ねこ;h]が好[す;a]き。` | Japanese | `Auto`, `detected` | `ja` confident |
| 5 | 1767261840000 | `！` · `ＡＢＣ` | Japanese | `Auto`, `detected` | `ja` confident |
| 6 | 1767261900000 | `pálida[pálido,adj,f]` · `La[el,det,f] gata[gato,n,f] es[ser,v,] pálida[pálido,adj,f].` | Spanish | `Auto`, `detected` | `es` confident |
| 7 | 1767261960000 | `Café` · `Ist das Café zu?` | Spanish (line 6, 1 min earlier) | `Fallback`, `recent` | none, uncertain |
| 8 | 1767262020000 | `sägen[sägen,v,]` · `Sie sägen.` | German | `Auto`, `detected` | `de` confident |
| 9 | 1767262080000 | `Peña` · `Peña und Kühn` | German (line 8) | `Fallback`, `recent` | none, uncertain |
| 10 | 1767262140000 | `bañarse[bañarse,v,]` · `¿Vas a bañarte?` | Spanish | `Auto`, `detected` | `es` confident |
| 11 | 1767262200000 | `Haus` · `Das Café ist zu.` | German | `Auto`, `detected` | `de` confident |
| 12 | 1767262260000 | `Berlin[Berlin,nnp,]` · `in[in,prep,\|in,adp,] Berlin` | German | `Auto`, `detected` | `de` confident |
| 13 | 1767262320000 | `streng[streng,adj,\|streng,adv,]` · `Streng vertraulich.` | German (line 12) | `Fallback`, `recent` | none, uncertain |
| 14 | 1767263400000 | `고양이` · `고양이가 있어요.` | German (line 12, 19 min) | `Fallback`, `recent` | `ko` uncertain |
| 15 | 1767265860000 | `Peña` · `Peña und Kühn` | German (line 12, exactly 60 min) | `Fallback`, `recent` | none, uncertain |
| 16 | 1767265920000 | `streng[streng,adj,\|streng,adv,]` · `Streng vertraulich.` | Spanish (line 12 is 61 min old) | `Fallback`, `fallback_profile` | none, uncertain |
| 17 | 1767265980000 | `猫` · `我有猫` | Japanese | `Auto`, `detected` | `ja` confident |

Lines 13 to 16 rely on D12: `Fallback` commits (lines 7, 9, 13, 14, 15) never move the last route, so from line 12 on it stays at line 12's time. The sentences are synthetic; none comes from the user's collection. Replaying the file must put every line in `correct` or `uncertain_correct` with no `expect_mismatch` (D14.3).

## D16. Differences from the Migaku add-on 0.6.6

| Behavior | Add-on | Meguri |
|---|---|---|
| Destination | the open Add window's note type and deck, else one configured note type and deck | profiles chosen by detection, override or fallback (D11) |
| Field map | per-note-type overrides in its config, else inferred from field names | stored per profile, inferred once by D4 |
| Defaults | none (the user's own hook filled two flags) | per profile (D5) |
| Tags | the Add window's, else none | per profile |
| Missing destination | (no case: one destination) | D7.1 errors, never a silent substitute |

No `conformance/deviations.yaml` entry is needed: there is no Migaku oracle (U136).

## D17. Open questions

Until each is settled, Meguri follows the rule of the section named.

1. The add-on's field-name rules are multilingual; its keyword lists are not in the permitted sources. D4 uses English words only, which is enough for the user's note types (R1712). Field names in other languages infer `none`.
2. Which tag Migaku syntax uses for German verbs. D10 counts the lemma-ending signal on the tags `v`, `verb` and `aux`; if German verbs carry another tag, the German ending signal never fires.
3. R1716 says Han characters mean Japanese. D9.1 sends Han-only text to a Chinese or Cantonese profile when no Japanese profile exists, and treats it as uncertain when both exist; R1716 names only the second case.
4. Settled. Routing records no `Uncertain` decision: every card that D11 step 4 routes is `Fallback` and is reported `uncertain` (D13.1), a confident language with no profile included. `servers/migaku-received-log` L4 shows R1722's labels from the decision and D13.1's detected language: `Fallback` with no detected language is labelled Uncertain, `Fallback` with a detected language (one that no profile has) is labelled Fallback; both count for the badge.
5. The stopword lists, the per-occurrence counting and the Latin letter lists of D10 are Meguri's own constants. Only U249's private run (R1718: 0 wrong, at most 0.5 % uncertain) can confirm them; a failing run changes this spec, not the gate.
6. The full list of languages the extension supports. D1 lists eleven; a payload in another language is handled as uncertain Latin or CJK text.
7. The cut-off that separates the 1,260 notes received through `/receive` from older ones (D14.1) is a private value of U249's run. It is the first card the log records as created through `/receive`, which is later than the first line of the log. The counts it must reproduce are R1947's.
8. Whether the received notes of one note type all sit in one deck. Routing gives each language the one deck of its draft (D6), so a note type whose received notes span several decks fails R1947 as `wrong_destination` (D14.3). The private run reports it; it then needs a spec decision (for example a second profile), not a changed gate.
9. Whether every received note still has the flag values its defaults gave. D14.3 compares the created note's defaulted fields with the expected profile's defaults, which is how AE87's "right default flags" is read here. A note whose flags the user changed after it was received is not a routing miss, so the source note's current flags are not compared.
10. Whether the history holds two received notes with no media and the same `targetWord` and `sentence` within 600 s. Live, Meguri answers the second as a retry (`servers/migaku-receive` P12), so the replay counts it as `retry_matched`, which fails both gates. The private run would show whether this case exists.
11. The D10.1 lists leave out common German and Spanish words of four letters or more (for example the German negation and the Spanish word for "but") because the private-token guard (R1940) refuses them in committed files while no committed file holds them. The lists are therefore shorter than lists of the most common words would be. The product owner may approve committing those words, which restores fuller lists; until then D10.1 stands as written, and U249's private run shows whether it meets R1718. For the same reason D1 names the `it` language through ISO 639-1 rather than spelling its English name.

## D18. What this file relies on from `servers/host-security`

| Relied on | Given by |
|---|---|
| the integrations store, with versioned migrations to which U138 adds its own tables: profiles (D2.1), the routing settings (fallback profile and override, D11) and the routing state (D12) | H10.1 |
| one clock for every request's arrival time, which D1's routing clock, D6's "now" and D14.3's replay share; a test clock in tests | H21 |
| the in-process server that `replay` runs (D14.3), on a given collection, with that clock and without a socket | H22 (test construction, socketless serving) |
| the FFI module that D2.3's operations join, returning `NotStarted` before the host starts and `Unsupported` on iPhone and iPad | H20 |

The FFI operations of D2.3 and the resolution of D13 are this file's.
