# UI strings for the stats screen, activity section and deck options (U225 to U237, U228a)

Spec-writer notes for `specs/decks/deck-options-strings.md` and
`specs/stats/activity-strings.md`, and a check of `specs/stats/graph-presentation.md` P19. Session s-20261110T085429Z-165d83d3
(2026-11-10). Read at tag 26.09.3 (`refs/anki-26.09.3`). Only English interface
strings were taken, as allowed for interface text; no code was copied.

| Spec section | Source | Read for |
|---|---|---|
| deck-options-strings K2 | `ftl/core/deck-config.ftl` (section titles), `ftl/core/scheduling.ftl` (New Cards, Lapses), `ftl/core/actions.ftl` (Auto Advance); section components under `ts/routes/deck-options/` (`DailyLimits.svelte`, `NewOptions.svelte`, `LapseOptions.svelte`, `DisplayOrder.svelte`, `BuryOptions.svelte`, `AudioOptions.svelte`, `TimerOptions.svelte`, `AutoAdvance.svelte`, `AdvancedOptions.svelte`) | which translation each section title shows |
| deck-options-strings K3 | the same `.ftl` files and section components; `ftl/core/scheduling.ftl` for New cards/day, Maximum reviews/day, Minimum interval, Leech threshold, Leech action, Show on-screen timer, Maximum interval | each option's English label; the trailing space of Question action's English |
| deck-options-strings K4 | `ftl/core/deck-config.ftl` (daily limit tabs), `ts/routes/deck-options/DailyLimits.svelte`, `ts/routes/deck-options/FsrsOptions.svelte` | tab names, and which options carry which tabs |
| deck-options-strings K5 | `ts/routes/deck-options/choices.ts`, `ftl/core/deck-config.ftl`, `ftl/core/decks.ftl` (Relative overdueness, Order added, Latest added first), `ftl/core/actions.ftl` (Suspend Card), `ftl/core/scheduling.ftl` (Tag Only), `ftl/core/studying.ftl` (Bury Card) | each choice's label, the list order, and the FSRS labels of the two ease orders |
| deck-options-strings K6 | `ftl/core/deck-config.ftl` (used-by text), `ts/routes/deck-options/ConfigSelector.svelte` | the picker's "name (used by N decks)" composition and its plural |
| activity-strings S2, S3 | no Anki file: the Review Heatmap add-on 1.0.1's wording as R1326, R1333 and R1339 give it, checked against the project's private research notes on the add-on; the add-on was not opened | streak labels, explanations and settings names |

Every P19 row was compared with `ftl/core/statistics.ftl` and
`ts/routes/graphs/TrueRetention.svelte` in the same session. All match except
the retention help row, which differs from Anki's English in one word; Anki's
word is blocked by the private-token scan (R1940), so the row was left as it is
and the difference stays open for a decision.

Review round (same session): the K5 menu order of Review sort order under
FSRS (6 then 5) was checked again in `ts/routes/deck-options/choices.ts`. The
K2 note on the missing FSRS section and the K6 and S1 argument forms come from
R736, R752, `sched/deck-options` O3 and `stats/heatmap-presentation` M10.3, not
from Anki. The Skip question key was renamed so that it follows the protobuf
field rather than Anki's translation id; the New card gather order key keeps
`newGatherPriority`, which is the stored deck config JSON key
(`model/schema11-json`), an interface name.
