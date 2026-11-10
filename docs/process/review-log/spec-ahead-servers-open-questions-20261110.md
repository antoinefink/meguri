# Servers specs: open questions for the product owner

Status: decision list, uncommitted spec-writer work (wave 2, `specs/servers/`),
updated after the wave-5 probe of the pinned add-on (25.11.9.0 in Anki 26.9.3,
2026-11-10).
Base: `994760c1a6211e2b48a5aa11da156a1a2d46afa4`.
Sessions: s-20261110T085429Z-165d83d3 (spec-writer), this finishing pass and
the wave-5 probe pass.

Each item gives the spec id, the question, the rule the spec follows until
it is answered ("Interim") and a recommendation. "Observed" gives what the
probe showed; "Settled" marks an item the probe closed. Blocking items come
first. Items that only a recording, a header capture, U147's manual checks
or U249's private run can settle are listed last per unit, without a choice.

## Blocking

- **U142, actions A21 Q2** (blocks A8). Missing-note text: the add-on's
  `Note was not found: <id>` (and `Card was not found: <id>`) against
  R1738's lower-case `note was not found: <id>`. Interim: the add-on's text,
  so goldens replay. `guiEditNote` keeps R1786's lower-case text either way
  (gui G7, G11; envelope E15.1). Observed: `Note was not found: 5` from
  `updateNoteFields`, `updateNote`, `updateNoteTags`, `getNoteTags`;
  `Card was not found: 7` from `suspend`, `unsuspend`, `suspended`.
  Recommend: amend R1738 to the add-on's text; no deviation.
- **U142, actions A21 Q3** (blocks A12.1, A12.2). `suspend` returns `true`
  for a list of two or more cards that are all suspended already, and
  `unsuspend` returns `null` in the add-on; R1767 says "true if anything
  changed". Interim: the add-on's results. Observed: A12.1's table
  (`[]` false; one already suspended false; two or more true; an unknown id
  after a suspended card true, otherwise `Card was not found`); `unsuspend`
  `null` in every case. Recommend: follow the add-on, amend R1767, and
  restate the U142 test as "a one-card list of an already suspended card
  returns false".
- Settled in this pass: received-log L18 Q4. `conformance/undo-ops` V4 now
  lists `move_received_card` (Move Received Card) as a Meguri-only step, so
  U140 is no longer blocked. U140 still adds `move_received_card` to the
  `undo_step` enum of `conformance/schema/observation.schema.json` and the
  `engine/op-model` O9 entry `migaku_move`.

## U141: AnkiConnect envelope

- **Envelope, E7.3.** `Access-Control-Allow-Origin` on a request without
  `Origin`: `*` (a reading of R1739) or the reference's `http://localhost`.
  Interim: `*`, listed in E18 as a difference. Observed: `http://localhost`
  on every no-Origin reply, the preflight and the 403 included; `*` on every
  reply while the list holds `*`. Recommend: `http://localhost`. No client
  reads it, it is the default allowed origin under R1739, and the E18 row
  goes.
- **Envelope, E7.2 rows 2-3, E9 rows 2-3.** Several `Origin` headers and
  `Origin: null` are refused even with `*`, and `requestPermission` shows
  no prompt for them. Interim: refused. Observed: with `*` the reference
  allows `null`, an empty and repeated origins, and grants
  `requestPermission` to `null`; without `*` the last header counts, `null`
  gets a prompt and can be deny-listed, an empty `Origin` gets a prompt, and
  Allow appends the value as sent (`null` or empty), so later requests with
  it are allowed. Recommend: confirm.
- **Envelope, E12.3.** `sync` and `guiAddCards` count as mutating, so
  read-only mode refuses them. Interim: both mutating. No probe applies.
  Recommend: confirm.
- **Envelope E12.3, gui G3.** Read-only mode refuses `guiBrowse` with
  `reorderCards`, since the sort is a config write. Interim: refused. No
  probe applies. Recommend: confirm.
- Settled by probe: E2 to E5, E4.1 integer edges, E5.3 and E18's 403 form
  (no `Content-Type`, no `Content-Length`, empty body, close), E7.2's
  localhost rule (case-sensitive), E7.4 (the PNA value is compared exactly;
  new E19 `preflight-pna-case`), E8 key order, E9 and E9.1 (concurrent
  requests from one origin get one prompt and one entry; the E18 row
  "appended again" removed), E10, E11 (a non-object element fails only when
  reached and earlier changes stay; new E19 `non-object-entry-later`), E12
  (121 names = E12.3's 73 + E12.4's 48). A HEAD reply carries the body
  (E2, E19 `head-request`, Meguri); header lines without a space after the
  colon are ignored (E18, H16). E15.1's observed texts are recorded; its
  policy is A21 Q2.

## U136: transcripts and clients

- **Clients C9.6** (may conflict with requirements). The `Origin` that the
  card webview sends: `meguri-media://media` or `null`. If `null`, R1679's
  opt-in cannot work, because R1744 refuses `null` whatever the list holds.
  Interim: both refused by default. No add-on probe answers it. Recommend:
  keep R1744; if U136 or U147 shows `null`, give the webview a non-opaque
  origin rather than accept `null`.
- **Transcripts T19.5.** Record replies with real client extension ids too?
  Interim: synthetic ids. Observed: the add-on treats `chrome-extension`,
  `moz-extension` and `safari-web-extension` ids alike. Recommend: no.
- Settled: T19.1 (the driver add-on's `allow`, `deny`, `deny_ignore` give
  T7.2's replies and list changes; a dialog with no queued answer holds
  every request,
  so `none` stays declared-only; a `requestPermission` that ends without a
  prompt consumes no answer). T19.3 (ids are the frozen clock in ms plus the
  lowest free offset per kind, collide across kinds and are reused after a
  delete; T10.1 binds tokens to entities with retired tokens, T10.2 adds the
  run error `reused_ref`). T9 `oracle.addon.sha256` is the archive hash of
  `ADDON_PIN`. C9.11 (`guiEditNote`; gui G11). T18's harness `version` and
  ping replies.
- Fact that no probe can settle: T19.2. The installed copy is gone; the pin
  to release 25.11.9.0 by archive hash and the 121-action match are
  recorded.
- New, for U136's recorder: C9.12, fix the browser's selection before
  recording `mpvacious/update-last-card`; record
  `mpvacious/find-models-by-name` again (`did` null on both sides);
  `asbplayer/request-permission-key` needs an API key in the add-on's
  config at startup and was not probed.
- Facts to record: C9.1 to C9.5 and C9.7 to C9.10 (client versions,
  headers, media forms, query forms, call orders, rows 40-47).

## U142: AnkiConnect actions (other)

- **A21 Q1.** PROFILE name. Interim: `User 1`. Observed: Anki's en_US first
  profile is `User 1`; the recorder reports its own profile name (`oracle`
  by default), so it must run as `User 1` if that is chosen. Recommend:
  confirm.
- **A21 Q4.** Review counts leave out review-log types 4 and 5 (R1774)
  while the add-on counts every row (observed, including the type-4 rows of
  `setDueDate` and `forgetCards`). Interim: exclusions, deviation
  `ankiconnect-review-counts-exclusions`. Recommend: confirm.
- **A21 Q5.** The A17 error texts are Meguri's; the add-on's texts are
  recorded in A18 `ankiconnect-error-text`. Recommend: accept.
- **A21 Q7.** R1758's "no card would be generated" never happens in Anki, so
  no refusal. Interim: no refusal. Observed: such a note is added with one
  card of ordinal 0. Recommend: confirm.
- **A21 Q8.** Cloze states keep `cannot create note for unknown reason`
  (observed text). Recommend: confirm.
- **A21 Q9** (new). A string `tags` in a note object: the add-on makes each
  character one tag (`"pq rs"` gives `p q r s`) and ignores some non-object
  `options` and `duplicateScopeOptions`. Interim: member errors of A7.1 and
  A7.4, deviation `ankiconnect-note-member-types`. Recommend: keep the
  member errors.
- **A21 Q10** (new). `areDue` and `getIntervals` for a card with no
  review-log row fail with the add-on's runtime text `list index out of
  range`. Interim: the add-on's text, so goldens replay. Alternative: an A17
  text with deviation `ankiconnect-no-revlog-text`. Recommend: none yet.
- **A18 `ankiconnect-canadd-no-media`** (new, from U143's probe). The
  add-on's `canAddNote` returns `false` when an attachment fails and has no
  `fields`; Meguri's check ignores attachments and returns `true`. A18
  should name that reply too. Recommend: add it to the same entry.
- Settled: A21 Q6 (the probe confirmed this file except the points now
  written into A7.1, A7.4, A10.1, A11.2, A11.4, A12, A13, A14, A18, A20;
  goldens still decide). New deviation `ankiconnect-nextreviews-isolation-marks`.
- Facts for U136's recorder setup: notes/preservation, cards/reschedule
  against the UI ops, search/current with cards in B, the too-deep deck
  error, and the step-remap and FSRS points of changedeck-op.

## U143: AnkiConnect media

- **X15 Q7** (conflicts with R1762). An attachment that fails and has no
  `fields` fails the note, as the add-on does (observed: `'fields'`,
  `["'fields'"]` for `addNotes`). Interim: fails the note; a non-list
  `fields` gives `invalid parameter: fields` (deviation
  `ankiconnect-media-attach-fields-type`, same decision). Recommend: follow
  the add-on; amend R1762 to exclude that case.
- **X15 Q8.** Ingest notices: the plan puts them in the activity log, but
  E16 has request entries only and no names (R1780). Interim: a separate
  host event (X11.9). Recommend: keep the host event and show names in
  U144's media folder view; add an E16 entry kind only if a count-only
  entry is wanted.
- **X15 Q9.** A moved folder suspends scans until it is picked again (new
  baseline). Add a relocation call? Recommend: not now.
- **X15 Q1, Q2, Q3, Q4.** Proposed X12 texts (the add-on's runtime texts
  are X13 `ankiconnect-media-error-text`); 256 MiB cap for `path` reads (not
  probed); copy only, never remove; dot-names skipped by the scan but
  ingested when referenced. Recommend: confirm all four.
- **X15 Q6.** The X8 label must match U144's screen (Settings ›
  Integrations). Recommend: U144 decides; X8 follows.
- Settled: X15 Q5 (every recorded action probed, X16; goldens still
  decide). By probe: X3.1 falsy sources, X3.2 base64 (Meguri refuses
  non-ASCII), X3.4 and X13 URL limits (status 200 only, 30 redirects, 10 s
  per connect and read), X4 traversal (the last path component is kept:
  `media/store-traversal` now expects `"collection.anki2"`), X10 attachment
  rules, new X13 entries `ankiconnect-media-case-variants` and
  `ankiconnect-media-non-files`.
- Fact for U147 or a macOS run: X13 names order on a case-insensitive
  volume.

## U146: AnkiConnect GUI

- **G2 rule 6** (blocks scenario `review-left`). The screen shown when a
  navigating action ends a review session is U150/U175's, which no spec
  defines. Recommend: U150/U175's spec defines it first.
- **G6, filtered deck.** `guiAddCards` with a filtered `deckName`. Interim:
  the panel opens and the Add panel refuses later. Not probed (needs a
  recorder setup step). Recommend: refuse at once with `addNote`'s
  filtered-deck error.
- **G2 rule 5.** Only navigating actions bring Meguri to front;
  `guiSelectCard` and `guiSelectedNotes` do not. Not observable offscreen.
  Recommend: confirm.
- **G2 rule 4.** Selection actions are refused while an unsaved sheet is
  open. The reference has no such check. Recommend: confirm (matches the
  U146 test).
- **G6, reply moment.** The reply goes out when the panel or the
  Replace/Keep prompt is shown. Observed: the reference replies `0` as soon
  as its Add window shows, and at once on a second call with the window
  open. Recommend: confirm.
- **G9.** The `sync:` texts other than `sync: auth not configured`
  (confirmed) are Meguri's; `SyncNow` has no safety timeout. Recommend:
  accept the texts; no timeout.
- **G2 rule 5, closed window.** With bring to front off, a closed main
  window opens without activating Meguri. Recommend: confirm.
- **G2 note.** A navigating action during review leaves the session
  (R1788), also for asbplayer's two `guiBrowse` calls after each update.
  Recommend: keep R1788.
- **G11, sort direction** (new). The first-direction-descending rule and
  sort keys were not probed: Anki holds the collection locked, and the
  unsortable-column notice would block the oracle. Needs a recorder step.
- Settled by probe: G3 `guiBrowse` texts (order checked before
  `columnId`), G4 and G5 selection, G6 check order, G7 (`guiEditNote` on an
  existing note fails in Anki 26.9.3; G11 row added, `edit-valid` now
  Meguri), G9 texts.

## U135: host security

- **H18.3.** The shared fetcher ignores system proxies; the AnkiConnect
  add-on uses them (observed: it honors `HTTP_PROXY`; the macOS system
  setting was not probed). Interim: direct connections. Recommend: keep
  direct.
- **H18.6.** 128 connections per server and the 10 s head timeout have no
  source. Observed: the add-on has no timeout for an idle connection (at
  least 30 s) and about 5 s for a stalled request; the connection limit was
  not probed. Recommend: confirm.
- **H18.7.** The servers listen after a failed first open, so clients get
  an unavailable error. Recommend: confirm.
- Facts for U147: H18.1 (Docker Desktop and loopback), H18.2 (`[::1]`
  probe; the add-on listens on IPv4 only), H18.4 (`Host` values; the add-on
  ignores `Host`), H18.5 (close after each reply; the add-on closes after
  every reply and sends `Content-Length` on 200 replies).
- Settled by probe: every H16 AnkiConnect row, plus a new row on malformed
  and stalled requests (the request line is not checked; a partial request
  holds every other connection until it completes or a 5 s gap passes).

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

## Strings specs (same wave, outside `specs/servers/`)

- **Settings label, R227 and R419.** The Space/Return preference: R227's
  "Space and Return answer Good", R419's quoted Anki label, or Anki
  26.09.3's own spelling (lower-case "enter"). Belongs to U160's Settings
  strings; not in reviewer-strings. Recommend: decide with U160.
- **Title case, R1200 and R899.** macOS writes menu items in title case;
  R1200 quotes "All fields" and R899 "Restore default name", while
  find-replace F2, flags-and-marked F4 and U217 use title case.
  Interim: browse-strings W2 and W6 follow the requirements (W7).
  Recommend: decide; title case needs R1200, R899 and U220 corrected or a
  design-contract exception.
- **Retention help, R1409.** Anki's word in the true-retention help text is
  refused by the private scan (R1940), so graph-presentation P19 keeps its
  reworded text, with no P21 row yet. Recommend: accept the rewording and
  add a P21 row.
- **Apostrophe in deck option names.** deck-options-strings K1 and K8 apply
  reviewer-strings S1's typographic apostrophe to Deck Options. Recommend:
  confirm.
- **R1499 wrong-login text.** Anki's sentence differs from R1499's literal;
  the Anki-server login UI is hidden (N12.4). Recommend: decide when that UI
  returns.
- Owner follow-ups (no decision): sched/deck-options O11 and O3 to point at
  deck-options-strings K2 to K6; heatmap-presentation M17 to cite
  activity-strings S2 and S3, whose intro should also name U165 and whose
  S4 leaves U157's summary streak line without a key; find-replace F7 and
  F9 to name U220, not U219; browse-strings W6 to drop "the Change Note
  Type mapping" (editing-strings E9 owns it) and to decide Create Copy's
  ellipsis with U183; screen-inventory `noteTypes.templates`,
  `noteTypes.fields`, `noteTypes.changeNoteType`, `deckOptions.options` and
  `stats.heatmapSettings` to match the strings specs; editor/paste P21's
  joined image label (R191); plan U228, U229, U232 to U234 Files to list
  the new strings specs.
- Settled: the zero studied-today line is R1298's (studied-today T2.3,
  plan U228 corrected); the U159 Edit menu and U153 toasts use
  reviewer-strings S5's undo forms; R926's sort-field label is Anki's
  quoted text (editing-strings E8).

## Process

- The similarity scan skips `specs/**` by config, so no servers spec was
  compared, and the AnkiConnect and Migaku corpus sources do not exist
  before U136. `tools/mg similarity` also has no `refs/anki` corpus in this
  worktree; the new strings specs need it run from the main checkout.
