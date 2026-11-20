# Round 8 and round 9 specs: Browse and Note Types (2026-11-20, session s-20261120T154704Z-1e3b6b1f)

No Anki source was read. The sections restate Antoine's round 8 and round 9 decisions (`docs/design/decisions.md`, 2026-11-20 rows `browser`, `home.deckList`, `noteTypes`; `docs/design/native-interface-contract.md` N21, N22) as Meguri rules over existing specs. Facts about Anki's data (field options stored in the note type, the review log kept when cards are removed) were taken from the existing specs `notetypes/records` N2, `editor/field-html` F1 to F3 and `notetypes/structure-changes` S6.

| Spec section | Anki file | Read for |
|---|---|---|
| search/filter-tokens (all sections) | none | Meguri UI and query rules; Anki text is read and written only through the existing grammar, canonical writer and builder |
| browser/workspace (all sections) | none | Meguri UI rules; operations keep their own specs |
| notetypes/workspace, notetypes/draft-save (all sections) | none | Meguri UI and save-flow rules |
| sync/meguri-provider#MP7 | none | Meguri provider policy |
| the "Changed 2026-11-20" lines in search/*, browser/*, notetypes/*, render/preview, stats/card-info | none | pointers to the sections above |
