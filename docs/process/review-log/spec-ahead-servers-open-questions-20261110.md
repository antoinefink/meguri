# Servers specs: open questions for the product owner

Status: decision list, uncommitted spec-writer work (wave 2, `specs/servers/`).
Base: `994760c1a6211e2b48a5aa11da156a1a2d46afa4`.
Sessions: s-20261110T085429Z-165d83d3 (spec-writer), this finishing pass.

Each item gives the spec id, the question, the rule the spec follows until
it is answered ("Interim") and a recommendation. Blocking items come first.
Items that only a recording, a header capture, U147's manual checks or
U249's private run can settle are listed last per unit, without a choice.

## Blocking

- **U142, actions A21 Q2** (blocks A8). Missing-note text: the add-on's
  `Note was not found: <id>` (and `Card was not found: <id>`) against
  R1738's lower-case `note was not found: <id>`. Interim: the add-on's text,
  so goldens replay. `guiEditNote` keeps R1786's lower-case text either way
  (gui G7, G11; envelope E15.1). Recommend: amend R1738 to the add-on's
  text; no deviation.
- **U142, actions A21 Q3** (blocks A12.1, A12.2). `suspend` returns `true`
  for a list of two or more cards that are all suspended already, and
  `unsuspend` returns `null` in the add-on; R1767 says "true if anything
  changed". Interim: the add-on's results. Recommend: follow the add-on,
  amend R1767, and restate the U142 test as "a one-card list of an already
  suspended card returns false".
- Settled in this pass: received-log L18 Q4. `conformance/undo-ops` V4 now
  lists `move_received_card` (Move Received Card) as a Meguri-only step, so
  U140 is no longer blocked. U140 still adds `move_received_card` to the
  `undo_step` enum of `conformance/schema/observation.schema.json` and the
  `engine/op-model` O9 entry `migaku_move`.

## U141: AnkiConnect envelope

- **Envelope, E7.3.** `Access-Control-Allow-Origin` on a request without
  `Origin`: `*` (a reading of R1739) or the reference's `http://localhost`.
  Interim: `*`, listed in E18 as a difference. Recommend: `http://localhost`.
  No client reads it, it is the default allowed origin under R1739, and the
  E18 row goes.
- **Envelope, E7.2 rows 2-3, E9 rows 2-3.** Several `Origin` headers and
  `Origin: null` are refused even with `*`, and `requestPermission` shows
  no prompt for them. Interim: refused. Recommend: confirm.
- **Envelope, E12.3.** `sync` and `guiAddCards` count as mutating, so
  read-only mode refuses them. Interim: both mutating. Recommend: confirm.
- **Envelope E12.3, gui G3.** Read-only mode refuses `guiBrowse` with
  `reorderCards`, since the sort is a config write. Interim: refused.
  Recommend: confirm.

## U136: transcripts and clients

- **Clients C9.6** (may conflict with requirements). The `Origin` that the
  card webview sends: `meguri-media://media` or `null`. If `null`, R1679's
  opt-in cannot work, because R1744 refuses `null` whatever the list holds.
  Interim: both refused by default. Recommend: keep R1744; if U136 or U147
  shows `null`, give the webview a non-opaque origin rather than accept
  `null`.
- **Transcripts T19.5.** Record replies with real client extension ids too?
  Interim: synthetic ids. Recommend: no; the add-on treats every extension
  id alike.
- Facts to record: T19.1 (recorder and the permission dialog), T19.2
  (`ADDON_PIN` against an upstream commit), T19.3 (ids under libfaketime);
  C9.1 to C9.5 and C9.7 to C9.10 (client versions, headers, media forms,
  query forms, call orders, rows 40-47).

## U142: AnkiConnect actions (other)

- **A21 Q1.** PROFILE name. Interim: `User 1`. Recommend: confirm.
- **A21 Q4.** Review counts leave out review-log types 4 and 5 (R1774)
  while the add-on counts every row. Interim: exclusions, deviation
  `ankiconnect-review-counts-exclusions`. Recommend: confirm.
- **A21 Q5.** The A17 error texts are Meguri's. Recommend: accept.
- **A21 Q7.** R1758's "no card would be generated" never happens in Anki, so
  no refusal. Interim: no refusal. Recommend: confirm.
- **A21 Q8.** Cloze states keep `cannot create note for unknown reason`.
  Recommend: confirm.
- Fact to record: A21 Q6 (`ADDON_PIN` goldens override this file).

## U143: AnkiConnect media

- **X15 Q7** (conflicts with R1762). An attachment that fails and has no
  `fields` fails the note, as the add-on does. Interim: fails the note.
  Recommend: follow the add-on; amend R1762 to exclude that case.
- **X15 Q8.** Ingest notices: the plan puts them in the activity log, but
  E16 has request entries only and no names (R1780). Interim: a separate
  host event (X11.9). Recommend: keep the host event and show names in
  U144's media folder view; add an E16 entry kind only if a count-only
  entry is wanted.
- **X15 Q9.** A moved folder suspends scans until it is picked again (new
  baseline). Add a relocation call? Recommend: not now.
- **X15 Q1, Q2, Q3, Q4.** Proposed X12 texts; 256 MiB cap for `path` reads;
  copy only, never remove; dot-names skipped by the scan but ingested when
  referenced. Recommend: confirm all four.
- **X15 Q6.** The X8 label must match U144's screen (Settings ›
  Integrations). Recommend: U144 decides; X8 follows.

## U146: AnkiConnect GUI

- **G2 rule 6** (blocks scenario `review-left`). The screen shown when a
  navigating action ends a review session is U150/U175's, which no spec
  defines. Recommend: U150/U175's spec defines it first.
- **G6, filtered deck.** `guiAddCards` with a filtered `deckName`. Interim:
  the panel opens and the Add panel refuses later. Recommend: refuse at once
  with `addNote`'s filtered-deck error.
- **G2 rule 5.** Only navigating actions bring Meguri to front;
  `guiSelectCard` and `guiSelectedNotes` do not. Recommend: confirm.
- **G2 rule 4.** Selection actions are refused while an unsaved sheet is
  open. Recommend: confirm (matches the U146 test).
- **G6, reply moment.** The reply goes out when the panel or the
  Replace/Keep prompt is shown. Recommend: confirm.
- **G9.** The `sync:` texts other than `sync: auth not configured` are
  Meguri's; `SyncNow` has no safety timeout. Recommend: accept the texts; no
  timeout.
- **G2 rule 5, closed window.** With bring to front off, a closed main
  window opens without activating Meguri. Recommend: confirm.
- **G2 note.** A navigating action during review leaves the session
  (R1788), also for asbplayer's two `guiBrowse` calls after each update.
  Recommend: keep R1788.

## U135: host security

- **H18.3.** The shared fetcher ignores system proxies; the AnkiConnect
  add-on uses them. Interim: direct connections. Recommend: keep direct.
- **H18.6.** 128 connections per server and the 10 s head timeout have no
  source. Recommend: confirm.
- **H18.7.** The servers listen after a failed first open, so clients get
  an unavailable error. Recommend: confirm.
- Facts for U147: H18.1 (Docker Desktop and loopback), H18.2 (`[::1]`
  probe), H18.4 (`Host` values), H18.5 (close after each reply).

## U137: Migaku protocol

- **P17.1** (blocks release, not the unit). The `Origin` of the extension's
  `/receive` and `/srs-check`. A content-script origin would be refused by
  default. Needs a header capture with U147 before release.
- **P17.8.** `ok: false` during a short Check Database. Interim: follows L6
  availability. Recommend: keep until a capture shows the extension stops
  sending.
- Facts to capture: P17.2 to P17.6 (methods, connect wait, error bodies,
  byte comparison, `/learning-statuses`). P17.7 is receive P20's.

## U138: Migaku routing

- **D17.11** (new, this pass). The privacy guard (R1940) refuses common
  German and Spanish function words of four letters or more that the
  collection holds and no committed file does. D10.1 now holds only words
  that pass, with the D8, D10.2, D15.1 and D15.2 examples reworded to keep
  their scores. Recommend: keep the short lists until U249's private run;
  if it shows misses, approve committing the left-out words.
- **D1, `it`.** The English name of `it` is written as "ISO 639-1's English
  name for `it`" for the same reason. Recommend: approve committing the
  name, then write it out.
- **D17.3.** Han-only text goes to a Chinese or Cantonese profile when no
  Japanese one exists, beyond R1716. Recommend: confirm.
- **D17.9.** AE87's "right default flags" is read as the profile's defaults.
  Recommend: confirm.
- For U249's run: D17.1, D17.2, D17.5 to D17.8, D17.10.

## U139: Migaku receive and syntax

- **P25 Q1.** Pending reply `{"success": true, "created": false}`. Interim:
  the switch stays off. Recommend: keep off until a recording.
- **P25 Q2, Q4.** R1705's match read as a subset of one earlier receive's
  ids; R1715 read as two messages. Recommend: confirm both.
- Facts to record: P25 Q3, Q5, Q6 (items without data, id uniqueness,
  timeout handling); syntax S10 Q1 to Q6 (the add-on's outputs).

## U140: received log

- **L18 Q7.** Label: Uncertain for no detected language, Fallback for a
  detected language with no profile (L4); the other reading keys on the
  route. Recommend: confirm L4.
- **L18 Q1, Q2, Q3, Q5.** Version mismatch as 200 `text/plain`; `/create`
  rows record Manual; sub-commands reply `OK`; `/create` into a filtered
  deck refused. Recommend: confirm all four until a recording.
- **L18 Q6.** Undo and redo of a move are inferred from the state they
  leave. Recommend: accept; a change-stream event naming the restored step
  can come later.

## Process

- The similarity scan skips `specs/**` by config, so no servers spec was
  compared, and the AnkiConnect and Migaku corpus sources do not exist
  before U136.
