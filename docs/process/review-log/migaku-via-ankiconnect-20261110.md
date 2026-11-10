# Migaku works through AnkiConnect (Antoine, 2026-11-10)

Antoine: "migaku actually is just anki connect so you can skip the extension
work. If ankiconnect is replicated then Migaku would work."

- Spec side: no capture of the Migaku extension's own traffic is needed.
  Open items in `specs/servers/migaku-*.md` that wait on an extension capture
  (for example protocol P17.1, the Origin of `/receive` and `/srs-check`) are
  not pursued.
- AnkiConnect fidelity carries the Migaku integration: the transcript work for
  U136/U141-U143 records Migaku's AnkiConnect call shapes with the other
  clients' (`servers/ankiconnect-clients`).
- The plan still lists U137-U140 (port 44432 endpoints, routing, `/receive`,
  received log) and their specs stay committed. Whether those units are dropped,
  reduced or kept is a plan change for Antoine and the orchestrator; this note
  records the decision only.
