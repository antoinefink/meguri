# The `heatmap` config key

Spec: stats/heatmap-config
Requirements: R146, R1336, R1337, R1338, R1343
Anki: 26.09.3
Scenarios: conformance/scenarios/stats/heatmap-totals/, conformance/scenarios/stats/heatmap-config/

The collection config key that holds the heatmap settings, in the format of the Review Heatmap add-on 1.0.1. The key is a row of the collection's config table, so it travels with the collection: CloudKit sync carries it between the user's Meguri devices (`specs/sync/cloudkit-records.md`), and a manual Anki transfer (KTD26) or a collection package carries it between Meguri and desktop Anki. Desktop Anki with the add-on and Meguri read and write the same key, so a setting changed on one shows on the other once the collection has reached it (in the add-on, from its next desktop session: K5 item 3). This file says how Meguri reads the key, the rule every write keeps (K2), and the writer (K6). The config table itself is `specs/storage/config-table.md`.

## K1. Key

- Key `heatmap` in the collection's config table: a JSON object.
- It is read with a typed read (`specs/storage/config-table.md` C3): a missing key, a value whose storage class is not BLOB, bytes that are not valid JSON, or JSON that is not an object all read as "no settings" (K3 rule 1).
- The add-on keeps other options outside the collection, per desktop profile (whether to show the heatmap on each screen, streak visibility). They are not in this key. Meguri keeps its own per-device equivalents (R1342), outside this spec.

## K2. Members

| Member | JSON type | Default | Meaning | Read by |
|---|---|---|---|---|
| `colors` | string | `"lime"` | the add-on's colour theme; Meguri ignores it (R1315) | nobody |
| `mode` | string, `"year"` or `"months"` | `"year"` | calendar mode: one year, or nine months | `stats/heatmap-addon` (U96) |
| `limhist` | integer, days | `0` (no limit) | history limit | U96 (`stats/counting-rules` N5) |
| `limfcst` | integer, days | `0` (no limit) | forecast limit | U96 |
| `limdate` | number | `0` (off) | ignore history before this date; U96 defines its unit | U96 (N5) |
| `limcdel` | boolean | `false` | leave deleted cards' reviews out of Home | `stats/counting-rules` N3 |
| `limresched` | boolean | `true` | leave rows without a rating out of every heatmap | N2 |
| `limdecks` | array of integers (deck ids) | `[]` | decks left out of Home, with their descendants | N3.3 |
| `version` | string | none | the add-on version that wrote the key, for example `"1.0.1"`; ignored | nobody |

Other members are ignored by readers. The writer keeps them (R1338, U96).

A write changes only the members the user changed, as `specs/storage/config-table.md` C6 says: every other member, `colors`, `version` and unknown members included, keeps its value and its JSON kind (numbers stay numbers, deck ids in `limdecks` stay integers), and the whole value is written again in C4's compact form, so spacing and member order may change (R1338). When the result is the same value as the stored one, nothing is written. A key that is absent stays absent until the user changes a heatmap setting (R1337, K3 rule 5). Decided 2026-10-27: the first write to an absent key, or to a value that reads as no settings (K1), starts from the defaults object below; and every write adds each K2 member the result lacks, with its default, whatever `version` holds (missing, older, `"1.0.1"` or later, not a string). So no write stores a key the add-on cannot read (K5 item 1), and no `version` comparison is needed: when `version` is missing or older, the add-on fills only missing members, so the added members change nothing for it. This goes beyond R1338's "only the changed fields": a write can add members the user did not change, but never changes or removes a member that is present. These are the only members a write adds.

The settings a user who never changed them has: `{"colors":"lime","limcdel":false,"limdate":0,"limdecks":[],"limfcst":0,"limhist":0,"limresched":true,"mode":"year","version":"1.0.1"}`.

## K3. Reading rules

1. With no settings (K1), every member takes its default.
2. A member that is missing takes its default. The others keep their stored values: `{"limresched": false}` means `limresched` false and every other member at its default (scenario `partial-config`). This holds whatever `version` says, where the add-on differs (K5).
3. A member whose JSON value is not of its type in K2 takes its default:
   - booleans are JSON `true` and `false` only;
   - integers are JSON numbers without a fraction or exponent, read as in `specs/storage/config-table.md` C3;
   - `mode` other than `"year"` or `"months"` reads as `"year"`;
   - for `limdecks`, a value that is not an array reads as `[]`, and array entries that are not integers are skipped. Duplicate ids are allowed.

   The add-on writes only the types in K2, so no oracle scenario covers rule 3. It is Meguri's rule for values other writers may leave.
4. Range rules for `limhist`, `limfcst` and `limdate` (negative values, a date in the future) are U96's (`stats/heatmap-addon`).
5. Reading never writes. Opening a collection, building the aggregates and querying them leave an absent key absent and a present key byte for byte as it was (R1337, R1295). The add-on does write on its first read (K5).
6. Readers take the key's value as it is now: after any change to it (a local write, an incoming CloudKit change, a manual Anki transfer or package import that replaces the collection, a raw write, another process), the next query uses the new values (`stats/counting-rules` N7).

## K4. What each setting affects

- The core's resident Home counts (`stats/counting-rules` N7) depend on the values of `limresched`, `limcdel` and `limdecks` after K3, and on nothing else in the key. A change that leaves these three values as they were (for example `mode` or `colors`) needs no new Home counts.
- `limresched` also applies to deck scopes (N4). `limcdel` and `limdecks` apply to Home only (R1340, R1341).
- `mode`, `limhist`, `limfcst` and `limdate` apply when a heatmap query runs (U96).
- No member affects the Stats screen or the studied-today values (R1343, N6).
- The rollover hour is not in this key. The heatmap uses the day clock's rollover hour (`stats/day-buckets` B1).

## K5. Where the add-on reads the key differently

Meguri's reading (K3) differs from the add-on's in three ways. No golden shows them: the oracle reads the key afresh at each observation and keeps the add-on's own writes out of the collection (`specs/conformance/stats-ops.md` H1).

1. **Missing members and `version`.** The add-on fills in its defaults for missing members only when `version` is missing or older than `"1.0.1"`, and then stores the completed object with `version` `"1.0.1"`. When `version` is `"1.0.1"` and a member is missing, the add-on fails and draws no heatmap. Meguri reads each missing member as its default, whatever `version` says (K3 rule 2). The add-on never stores such a key itself. So a writer, Meguri's included (U96), must never store a key that has `version` `"1.0.1"` or later and lacks a K2 member. Meguri's writer meets this by storing every K2 member on each write (K2).
2. **Writes on first read.** The first time the add-on reads the key in a desktop session, it stores one: its defaults when the key is absent, or the completed object of item 1. So a collection that desktop Anki with the add-on has opened usually holds a complete key, which reaches Meguri with the collection (a manual Anki transfer or a collection package). Meguri's reading never writes (K3 rule 5, R1337). Only the writer of U96 does.
3. **When a change takes effect.** The add-on reads the key once per desktop session and keeps that value, so a value that reaches the desktop collection while Anki runs (from an Anki sync server that Meguri uploaded to) shows only after Anki restarts. Meguri uses the current value at the next query (K3 rule 6).

## K6. Writing the key (U96, U229)

The one writer of the key: an engine operation that the Settings page (U229, U160) calls when the user changes a heatmap setting. It follows K2's rule, which this section turns into steps. Nothing else writes the key (K3 rule 5, `stats/heatmap-addon` A12).

### K6.1 Input

A patch: one or more of the members below, each with the value the user chose. `colors`, `version` and unknown members cannot be in a patch.

| Member | Accepted values |
|---|---|
| `mode` | `"year"` or `"months"` |
| `limhist`, `limfcst` | an integer from 0 to 2,147,483,647; 0 is no limit |
| `limdate` | 0 (off), or an integer from 1 to 253,402,300,799: epoch seconds of the first instant of the chosen local date in the device zone, its local midnight or the first instant after a gap there (`stats/heatmap-addon` A2.2 reads it back as that date; a device in another zone can read the day before or after, `stats/heatmap-addon` A15 item 1). The Settings page offers dates from the collection's creation date to today (U229) |
| `limcdel`, `limresched` | `true` or `false` |
| `limdecks` | a list of deck ids, each an integer from 1 to 9,223,372,036,854,775,807, in the order given; it may be empty and may name decks that no longer exist (`stats/counting-rules` N3.3) |

A patch with no member, a member not in this table, or a value outside it is refused with `invalid_heatmap_setting` naming the member. A refused patch writes nothing.

### K6.2 Steps

All steps run in one write transaction, so a change that lands in between (a CloudKit change, another process) is either before the read or after the write, never lost.

1. **Drop what does not change.** Read the key's current values (K3). Drop each patch member whose value equals its current value (`limdecks`: the same ids in the same order). When nothing is left, the operation ends and writes nothing: an absent key stays absent (R1337), a present one keeps its bytes, `usn` and `mtime_secs`.
2. **Start.** When the stored value reads as an object (K1), start from that object as stored: every member with its value and its JSON kind. Otherwise (absent, not a BLOB, not JSON, not an object) start from K2's defaults object, `version` `"1.0.1"` included. A value that is not an object is replaced: it holds no setting any reader can use.
3. **Apply.** Set each remaining patch member to its new value: `mode` as a JSON string, the integers as JSON integers (never with a fraction or exponent), the booleans as JSON booleans, `limdecks` as a JSON array of integers. A member stored with a wrong type is replaced by the new value of the right type. Every other member keeps its value and kind (`specs/storage/config-table.md` C6), `colors`, `version` and unknown members included.
4. **Complete.** Add each K2 member the object lacks, with its default, except `version`, which has no default: an existing object without `version` stays without it. The add-on then fills its own defaults, which are the values just added (K5 item 1).
5. **Write** the object per `specs/storage/config-table.md` C4 in C5's compact form, members sorted by name. When it is the same value as the stored one, C4 writes nothing. Otherwise the row gets `usn` −1 and `mtime_secs` now, and `col.mod` moves (C4).

The write is not an undo step, like the other Settings writes (R115, as `specs/editor/config-keys.md` K1 applies it). It reaches the user's other devices by CloudKit like any config change (`specs/sync/cloudkit-records.md`), and the change stream reports the key, so the resident values follow (`stats/counting-rules` N7; K4 says which members need new Home counts) and the widget snapshot is rebuilt (`stats/widget-snapshot`).

### K6.3 Examples

| Stored | Patch | Written |
|---|---|---|
| absent | `mode` `"months"` | `{"colors":"lime","limcdel":false,"limdate":0,"limdecks":[],"limfcst":0,"limhist":0,"limresched":true,"mode":"months","version":"1.0.1"}` |
| absent | `mode` `"year"` | nothing; the key stays absent (step 1) |
| `{"colors":"flame","mode":"year","limhist":0,"limfcst":0,"limdate":0,"limcdel":false,"limresched":true,"limdecks":[],"version":"1.0.1","zz":{"k":1}}` (AE56) | `mode` `"months"` | `{"colors":"flame","limcdel":false,"limdate":0,"limdecks":[],"limfcst":0,"limhist":0,"limresched":true,"mode":"months","version":"1.0.1","zz":{"k":1}}` |
| `{"limresched": false}` | `limhist` 90 | `{"colors":"lime","limcdel":false,"limdate":0,"limdecks":[],"limfcst":0,"limhist":90,"limresched":false,"mode":"year"}` |
| `{"limresched":"no","version":"1.0.1"}` | `limresched` `true` | nothing: `"no"` reads as the default `true` (K3 rule 3), so step 1 drops the member |
| `{"limresched":"no","version":"1.0.1"}` | `limresched` `false` | `{"colors":"lime","limcdel":false,"limdate":0,"limdecks":[],"limfcst":0,"limhist":0,"limresched":false,"mode":"year","version":"1.0.1"}` |
| complete key with `"limdecks":[1894701600000]` | `limdecks` [1894701600000, 1894701600001] | the same with `"limdecks":[1894701600000,1894701600001]` |
| complete key with `"limdate":1788213600.5` | `limcdel` `true` | the same with `"limcdel":true`; `limdate` stays `1788213600.5` |
| bytes that are not JSON | `limcdel` `true` | the defaults object with `"limcdel":true` |

### K6.4 Tests

The writer has no oracle: its tests are Meguri's (U96 `heatmap_config_roundtrip`), one per row of K6.3, plus: opening, querying and closing a collection with no key writes nothing and leaves the sync-pending state as it was (AE56); every written value reads back through K3 as the patched values; two equal patches in a row write once. One oracle scenario checks that the add-on accepts the writer's output: `conformance/scenarios/stats/heatmap-config/writer-output` stores K6.3's first and third results with `db_exec`, with some rows and due cards, and observes `heatmap_report` (`specs/conformance/stats-ops.md` H11, which lands with `stats/heatmap-addon` A16's scenarios). Each observation must succeed, where a key the add-on cannot read makes it fail (K5 item 1), and its history and forecast must be those of the stored limits (none in both results). Decided 2026-10-27: H11 does not observe `mode` or `colors`, so the scenario does not check them; desktop Anki drawing the months timeline in flame colours (AE56's desktop half) is a manual check of the final evidence.
