# Migaku Recently received log, moves and legacy `/create` (U140)

Spec-writer notes for `specs/servers/migaku-received-log.md` L1-L18 and C1-C5
(session s-20261110T085429Z-165d83d3). No Anki, AnkiConnect or Migaku add-on
or extension code was read. The log, its statuses, the move rules and the
retry flow are Meguri's design, from the plan. The `/create` interface comes from
the plan and the research dossier's prose; what they leave open is listed in
L18. Collection operations (add, delete, Change Deck, undo) are composed from
the existing specs cited, whose provenance covers Anki 26.09.3. All examples
are synthetic.

Versions: the dossier describes the `/create` route of the Migaku add-on 0.6.6
and of the extension bundles 1.30.16.0 and 1.31.0.17.

| Spec section | Source | Version | Read for |
|---|---|---|---|
| header | plan R1680, R1731 | extension 1.30.16.0, 1.31.0.17; add-on 0.6.6 | `/create` is legacy; the add-on is the reference for its form |
| L1, L2 | plan R1665, R1721, R1722, U135 and U140 Approach; `specs/servers/host-security.md` H10.1; `specs/servers/migaku-routing.md` D13.1 | Meguri design | storage beside the collection, migrations, row parts (route, confidence and hidden flag included) |
| L3, L4 | plan R1719, R1722, R1727, U138 and U140 Approach; `specs/servers/migaku-routing.md` D11, D13.1, D17 item 4 | Meguri design | statuses, decision kinds, the label table (keyed on the detected language since the consistency pass of 2026-11-10; L18 Q7) |
| L5 | plan R1702, R1721, U140 Approach and test scenarios | Meguri design | 500 rows or 30 days, 7-day payloads from the failed instant, pending never pruned, drain before prune |
| L6 | plan R1859, U145 Approach; `specs/servers/host-security.md` H11.1 | Meguri design | paged reads, hidden rows, refresh events |
| L7 | Meguri design (avoids a duplicate after a client retry); `specs/servers/migaku-receive.md` P12 | Meguri design | superseded Failed rows |
| L8 | plan U140 Approach; `specs/engine/change-stream.md` S3 | Meguri design | change-stream subscriber, full reconcile |
| L9 | plan R1719, R1723, AE71, U140 Approach and test scenario "Badge" | Meguri design | badge rule (Uncertain and Fallback labels, Failed), Confirm |
| L10-L13 | plan R1724-R1727, AE74, U140 Approach (move history `{op seq, from nid, to nid, from deck}`); `specs/browser/bulk-actions.md` B7.2; `specs/notes/note-records.md` D1; `specs/engine/undo.md` M1, M4, M5, M11; `specs/engine/op-model.md` O9; `specs/conformance/undo-ops.md` V4 | Meguri design | eligibility, deck moves, cross-note-type moves, field copy and defaults, one undo step, the undo and redo patterns, routing state |
| L14, L15 | plan R1728, AE73, U140 Approach | Meguri design | Retry and its results, delete, Browse and Edit |
| L16 | plan U140 Files and Approach, R1859 | Meguri design | the FFI calls and their results |
| L17 | plan AE73, AE74, U140 test scenarios | Meguri design | scenario list |
| L18 | plan R1722, R1731, R1733; `specs/conformance/undo-ops.md` V4; `specs/engine/change-stream.md` S3; `specs/notes/deck-placement-and-position.md` D2 | add-on 0.6.6 for Q1, Q3, Q5; rest Meguri design | what the sources leave open |
| C1-C3 | plan R1731-R1733; `local-data/research/09-ankiconnect.md` §3.1 (`/create` row); `specs/servers/ankiconnect-gui.md` G8, G10 (`BrowseNote`) | add-on 0.6.6; extension 1.30.16.0, 1.31.0.17 | `version=2`, the `card` JSON keys (`noteTypeId`, `deckId`, `fields[{name, content}]`, `tags`), multipart `files`, `{"id": nid}`, the version-mismatch text, the sub-command names. Repeated fields, file-part selection, path separators, the filtered-deck refusal and the row's text are Meguri rules |
| C4 | plan R1713, R1721, R1732 | Meguri design | the two Meguri additions; no claim about add-on behavior |
| C5 | plan U140 test scenarios | Meguri design | scenario list |
