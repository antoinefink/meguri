# specs/servers/migaku-protocol.md (U137)

Spec-writer session s-20261110T085429Z-165d83d3. Written in Meguri's own
words from the sources below; no code, regular expression or asset of the
Migaku extension, the Migaku add-on, its web framework or Anki was copied or
translated. The Migaku extension and add-on sources themselves were not read
in this session: every protocol fact comes from the research dossier, which
recorded them from the installed extension bundles (1.30.16.0, 1.31.0.17),
the add-on 0.6.6, its log and `lsof`.

Anki 26.09.3 (`refs/anki-26.09.3`): not needed. The protocol has no Anki
behavior; collection behavior is cross-referenced to existing specs.

| Source | Read for | Spec sections |
|---|---|---|
| `local-data/research/09-ankiconnect.md` §0 | the extension talks only to port 44432; its hard-coded routes; WebSocket path `/anki-connect`; add-on binds all interfaces and sends `*` CORS | P1, P4, P15 |
| `local-data/research/09-ankiconnect.md` §3.1 | route list and methods (`GET /srs-check`, `POST /receive`, `POST /create`, WebSocket, `/srs-import*` GET/POST), the three CORS header values, `OPTIONS` 204, `/srs-check` reply and its `version` "4" requirement, WebSocket `{id, msg, data}` messages, add-on push commands, extension retry every 10 s, older routes and `/anki-hello` reply | P4, P5, P6, P9, P10, P11, P12, P15, P17 |
| `local-data/research/09-ankiconnect.md` §3.2 | reply shapes `{"success": …, "error": …}` and HTTP 400 failures, strings the extension special-cases | P8 |
| `local-data/research/09-ankiconnect.md` §5 | Meguri v1 recommendation (accept and hold WS, `ok` without WS) | P9, P10 |
| `local-data/research/09-ankiconnect.md` header (evidence list) | the extension builds 1.30.16.0 and 1.31.0.17 | header |
| `local-data/research/01-user-collection.md` §8 | add-on 0.6.6 route list incl. `/learning-statuses`, and its statement that the extension queries `/learning-statuses`, which contradicts 09 §3.1 (recorded as P17 item 6) | P4, P17 |
| `specs/engine/lifecycle.md` L1, L6 | lifecycle states; availability for the local servers (Ready and slot free, or pre-phase `CloudAdopt`) | P1, P9 |
| `specs/servers/migaku-receive.md` P14, P20 | the busy test `/receive` uses (Check Database counts as busy); Created rows that set `last_received_at` | P1, P9, P10.5 |
| RFC 6455 (public standard) §5.1, §5.2, §5.4, §5.5, §7.1.5, §7.4, §8.1 | masking, reserved bits, opcodes, control-frame limits, fragmented messages, close codes and their validity, UTF-8 rule | P10.1, P10.2, P10.4 |
| `docs/plans/research/rust-stack.md` §4 | WebSocket ping handling and generous message size; no `Json` extractors; hand-rolled CORS layer incl. private-network reply | P5, P10 |
| plan, Product Contract "Mac integrations" R1669, R1672, R1675–R1690; U135, U137 | requirements and the 400 over-limit reply that U137 asks the spec writer to confirm | all |

Meguri decisions made here (no external source): compact JSON; `Vary: Origin`;
`Allow` on 405; the 404/405/400/500/503 message strings; GET and POST on
`/srs-import*` and `/anki-hello`; `HEAD` → 405 with no body; the
`Connection: close` rule for unread bodies (P2); the 64-socket cap; close
codes 1001 and 1009, and 1002/1007 as RFC 6455 asks; the close-echo table of
P10.4; the `Origin` parsing details of P3.1, including LDH labels; the bare domain
`migaku.com` allowed, reading R1683's `*.migaku.com` as extension host
patterns are read; `ok` following L6 availability, a reading of R1684's "the
collection is open" (P9, P17 item 8); the definition of `last_received_at`
(P10.5); the 500 on a caught panic as an exception to R1702 (P8.2); the
list of expectations on `servers/host-security` (P18).

The example extension origins are invented: the Chrome identifier
`abcdefghijklmnopabcdefghijklmnop` only has the Chrome form (32 letters `a`
to `p`); no identifier of an installed extension appears in the spec.
