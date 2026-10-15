# Private collection results

Nothing from the user's collection is in this folder, and nothing ever will be. This README says where results from the collection go and what they may hold (R1938–R1943).

## Where things live

| What | Where | Committed |
|---|---|---|
| The user's collection (`collection.anki2`, its `-wal`, `collection.media/`) | `local-data/anki/` of the main checkout | never |
| Per-run copies, the vault's access log, the suite's private log | `build/private/<run>/` | never |
| The private token set | `build/private/tokens/` | never |
| The anonymized clone (`tools/mg anonymize`), its report and its salt | `build/private/<run>/clone/`, `build/private/anonymize.salt` | never, unless Antoine decides otherwise |
| Private suite summaries | `conformance/reports/private/<name>.json` | yes, after the scan |

Only `tools/mg private` and `tools/mg anonymize` (by default) read the collection. They run in the main checkout, refuse CI machines and implementer or fixer worktrees, and copy the collection through a read-only mount in a container without a network. They open the collection database, its write-ahead log and the media folder, and nothing else in the profile folder.

The anonymized clone (`specs/fixtures/anonymizer.md`) holds none of the collection's text, deck names, tags or media names, and Anki schedules it exactly as the collection. It is still derived from the collection, so it stays on this Mac.

## What a committed result may hold

Aggregates and diffs keyed by logical ids only: pass and fail counts, agreement percentages, timing ratios and ids that mean nothing outside the collection. Never field text, deck names, tags or media names (R1940).

A suite summary (`tools/mg private suite --record <name>`):

```json
{
  "checks": {
    "next-states": {"failed": 0, "passed": 57600},
    "queues": {"error": true}
  },
  "report_version": 1
}
```

A check that could not run is `{"error": true}`; its error text stays in `build/private/<run>/suite.log`.

## The output guard

`tools/mg private copy` rebuilds the token set from each copy: deck names, tags, media names and field words of four or more characters, minus anything already in the repository's tracked files.

- `tools/mg private suite` scans its summary and writes it here only when the scan is clean.
- `tools/mg private scan-outputs <paths>` checks any file before it is committed, and `tools/mg private scan-outputs` with no path checks the staged lines.
- The pre-commit hook (`tools/hooks/pre-commit`) blocks a commit that adds a line holding a token.

Each of them names files and line numbers, never the token.
