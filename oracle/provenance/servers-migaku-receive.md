# Migaku `/receive` pipeline (U139)

Spec-writer notes for `specs/servers/migaku-receive.md` P1-P25 (session
s-20261110T085429Z-165d83d3). No Anki, AnkiConnect or Migaku add-on or
extension code was read. The interface facts come from the plan and the
private research dossiers, which summarize the add-on 0.6.6 sources, its log
and the extension bundles 1.30.16.0 and 1.31.0.17 in prose. Collection
behavior (note add, media store, undo, lifecycle) is not restated: it is
composed from the existing specs cited in each section, whose own provenance
covers Anki 26.09.3. There is no Migaku oracle (plan U136), so the scenario
tables of P24 are spec-derived. No user payload, note text or media was
copied; all fixture text and media stubs are synthetic.

Versions: "add-on 0.6.6" and "extension 1.30.16.0 / 1.31.0.17" below are the
versions `local-data/research/09-ankiconnect.md` describes (its evidence list
and §0).

| Spec section | Source | Version | Read for |
|---|---|---|---|
| header, P1 | `local-data/research/09-ankiconnect.md` evidence list and §0 | extension 1.30.16.0, 1.31.0.17; add-on 0.6.6 | extension bundle versions, add-on version, the hard-coded `127.0.0.1:44432/receive` |
| P2 | plan U139 Approach | Meguri design | stage order |
| P3, P4 | plan R1691, R1692; `09-ankiconnect.md` §3.2 (payload example) | extension 1.30.16.0, 1.31.0.17 | key names, item shapes (`id`, `title`, `input`, `r2Url`, `uploadId`, `src`, `alt`), no deck/note type/language; type handling and the stage-4 data check are Meguri's |
| P5 | plan R1710, R1715, U138 Approach; `specs/servers/migaku-routing.md` D11, D13.1 | Meguri design | the plan's parts (route and confidence included), the refusal cases |
| P6 | plan R1693; `09-ankiconnect.md` §3.2 steps 1-2 | add-on 0.6.6 | source keys, `[sound:…]` and `<img src=…>` forms, the `\n<br>\n` joiner, first/rest images |
| P7 | plan R1694, R1703, R1711-R1713; U139 test scenarios | Meguri design | verbatim HTML, defaults when empty, `No fields to map to.`; the "nothing to add" case is Meguri's (U139 asks the spec writer to fix it) |
| P8 | plan R1695, R1701; plan open question "Remote media destinations"; `specs/servers/host-security.md` H8, H13; RFC 2397 and RFC 3986 (scheme case) | Meguri design | data URLs and http(s), 5 redirects, 8 s, parallel fetch, no destination restriction, the fetch-failure mapping |
| P9 | plan R1695 | Meguri design | the MIME examples and the rule order (MIME first, magic bytes only for a missing or generic MIME); the full tables and magic signatures are Meguri's, from the public container formats |
| P10 | plan R1696, R1698-R1700; `specs/media/media-files.md` M3, M4, M13 | Meguri design | name reduction, random 11-hex names, reuse and conflict rename |
| P11 | plan R1697, U135 and U139 Approach (AudioTranscoder, Linux pass-through stub); `specs/servers/host-security.md` H11.3 | Meguri design | normalization contract |
| P12 | plan R1705, U139 Approach step 5; `specs/servers/migaku-received-log.md` L3 | Meguri design | the 10-minute window, matching on raw non-empty ids, text pair; the equal-text condition on media matches and the live-note reply are Meguri's extra conditions (Q5) |
| P13 | plan R1707, U139 Approach step 5; `specs/engine/op-model.md` O2 | Meguri design | FIFO, arrival order, off main thread |
| P14, P15 | plan R1706, R1797, U139 Approach step 7, Deferred to Planning ("the Migaku pending reply: verified before use, else HTTP 400"), Open Questions; `09-ankiconnect.md` §3.2 step 6 (intercept-mode reply, add-on 0.6.6); `specs/engine/lifecycle.md` L4-L6; `specs/servers/migaku-routing.md` D7.2 | add-on 0.6.6 for the intercept reply; rest Meguri design | the 5 s wait, the switch, "Meguri is busy", the pending queue and its frozen plan |
| P16, P17 | plan R1702, R1704, R1789, R801, U139 Approach steps 5-6, U139 test scenario "SIGKILL"; `specs/notes/note-records.md` A1-A4; `specs/engine/op-model.md` O9; `specs/engine/undo.md` M11 | Meguri design | one op, caller GUID, intents, hidden rows, no add-default writes, catalog entry |
| P18, P19 | plan R1702-R1704, R1715; `09-ankiconnect.md` §3.2 step 6 (strings the extension special-cases, timeout errors included) | extension 1.30.16.0, 1.31.0.17 | reply shapes and the reused strings; the other messages are Meguri's |
| P20 | plan R1702, R1709, R1721, U139 Approach; `specs/servers/host-security.md` H11.1; `specs/servers/migaku-protocol.md` P8.2, P17 item 7 | Meguri design | row writes, events, toast, the caught-panic row |
| P21 | plan R1729, R1730, U139 Approach step 8 ("blank-front result with no AV tags"); `specs/render/card-render-pipeline.md` K4 | Meguri design | the three warnings |
| P22 | plan R1948, U136 Approach; `specs/servers/transcripts.md` T14 | Meguri design | equivalence check |
| P23 | plan R1673, R1683, R1696, R1706, R1705, Assumptions (pending reply); `09-ankiconnect.md` §3.2 steps 3, 6 | add-on 0.6.6 | differences from the add-on (Add window, intercept, ffmpeg) and from R1706; retry detection and leftover media of the add-on are not described by any source and are marked "not known" |
| P24 | plan AE72, AE73, U139 test scenarios | Meguri design | scenario list |
| P25 | plan R1691, R1705, R1706, R1715, Open Questions; `09-ankiconnect.md` §3.2 step 6 (intercept reply, timeout errors) | add-on 0.6.6; extension 1.30.16.0, 1.31.0.17 | Q1 (the add-on sends `created: false` only in intercept mode), Q3, Q5, Q6 |
