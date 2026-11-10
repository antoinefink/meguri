# Activity strings

Spec: stats/activity-strings
Requirements: R1326, R1333, R1339, R1442
Anki: none (the texts are the Review Heatmap add-on 1.0.1's names, as R1326, R1333 and R1339 quote them; Anki 26.09.3 has no such text)
Scenarios: none (screen text; U228 and U229 test it, S5)

The texts of the activity section (U228) and of the Heatmap settings group (U229) that keep the add-on's wording, so users who know the add-on find the same names: the four streak stats with their explanations, and the add-on's settings. U228 and U229 take these texts only from this file. The heatmap's own texts (tooltips, navigation, accessibility) are `stats/heatmap-presentation` M17; the studied-today line is `stats/studied-today` T8. Everything else in these places is Meguri's (S4).

## S1. Keys and form

- Every string is an entry of the Stats String Catalog with the stable key given here, as in `stats/heatmap-presentation` M17. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped.
- `%lld` is an integer argument. `(s)` marks a plural on the number before it: English uses the singular for exactly 1 and the plural otherwise, 0 included.
- Counts are written as `stats/heatmap-presentation` M10.3 writes them (the locale's integer format with grouping, `1,234`). As in M17, each `%lld` below only chooses the plural form: the key takes the count for its plural rule and the written count as a separate text argument, which is what shows where the table has `%lld`.

## S2. Streak stats (R1326, R1333, R1442)

Four pairs, in this order. The value follows its label after a space; the label is in regular weight and the value in bold (R1326), tinted by the levels of `stats/heatmap-addon` A8.2 (R1332). The values are A8.1's.

| Key | English | Where |
|---|---|---|
| `activity.streak.dailyAverage.label` | Daily average: | first pair |
| `activity.streak.dailyAverage.value` | %lld card(s) | its value |
| `activity.streak.dailyAverage.help` | Average reviews on active days | its explanation |
| `activity.streak.daysLearned.label` | Days learned: | second pair; its value is a format, not a string (below) |
| `activity.streak.daysLearned.help` | Percentage of days with review activity over entire review history | its explanation |
| `activity.streak.longest.label` | Longest streak: | third pair |
| `activity.streak.longest.value` | %lld day(s) | its value |
| `activity.streak.longest.help` | Longest continuous streak of review activity. All types of repetitions included. | its explanation |
| `activity.streak.current.label` | Current streak: | fourth pair |
| `activity.streak.current.value` | %lld day(s) | its value |
| `activity.streak.current.help` | Current card review activity streak. All types of repetitions included. | its explanation |

- Examples: `Daily average:` **`1,234 cards`**, `Days learned:` **`85%`**, `Longest streak:` **`1 day`**, `Current streak:` **`0 days`**.
- The explanation shows on hover on the Mac and on long press on iPhone and iPad (R1333), and is the pair's VoiceOver hint (R1442). VoiceOver reads the pair as one element: the label text as its label, the value text as its value.
- The Days learned value has no catalog key and no plural: it is the whole percentage from 0 to 100 written in the locale's percent format with no fraction digits, so the locale places the sign and any space: `85%` in English, the number then a narrow no-break space and the sign in French, the sign first in Turkish.

## S3. Heatmap settings (R1339)

The add-on's names for the options under the group's Advanced disclosure.

| Key | English | Setting (`stats/heatmap-config`) |
|---|---|---|
| `heatmapSettings.historyLimit` | History limit | `limhist`, in days |
| `heatmapSettings.forecastLimit` | Forecast limit | `limfcst`, in days |
| `heatmapSettings.noLimit` | No limit | the value 0 of either limit |
| `heatmapSettings.ignoreBefore` | Ignore data before | `limdate` |
| `heatmapSettings.excludeDeleted` | Exclude deleted cards from history | `limcdel` |
| `heatmapSettings.excludeRescheduled` | Exclude manual reschedules from history | `limresched` |
| `heatmapSettings.excludeDecks` | Exclude decks from main heatmap | `limdecks` |

## S4. Meguri's texts (not listed here)

Meguri's own, from Meguri's String Catalog: the group name `Heatmap`, Calendar mode and its choices Year and 9 months (R1339 names them; the add-on's own names differ), the Advanced disclosure, the unit after a limit, the three device-local toggles (R1342), the message that replaces the heatmap and streaks when the scope has no history (R1309 gives its text), the deck page's counts labels (`docs/design/native-interface-contract.md` N11.5, N12.1), the explanation of the "+N" difference (R71, R1345), the deck page's Stats action (R1347), and the session summary's streak line (R95).

## S5. Tests

No scenario: these texts are not core output. U228's String Catalog test compares the English of every S2 key with the table and checks the plural forms at 0, 1 and 2 and the grouped count at 1,234; a value test checks Days learned in English and French. U229's String Catalog test compares every S3 key with its table.
