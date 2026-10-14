# Critic calibration examples

Labelled examples from Antoine's decisions, given to every critic session as few-shot calibration (R1984). Each file is one example: a critic issue (or a missed problem) and Antoine's verdict on it. `tools/mg critic` copies this folder, with `../decisions.md`, into the packet's `calibration/`.

Examples come only from Antoine's own decisions: gallery reviews (`../reviews/<date>.json`), variant picks and vetoes (`../decisions.md`) and chat. Agents never write an example from their own judgement. U254's `tools/mg variants` turns new decisions into examples.

One JSON file per example, named `<YYYY-MM-DD>-<screen>-<rule>.json`:

```json
{
  "date": "2026-11-02",
  "screen": "home.deckList",
  "variant": "dark-ax5-iphoneSmall",
  "rule": "text-truncation",
  "critic_said": {"severity": "high", "summary": "Deck name cut off after 'Japanese Vo…'"},
  "verdict": "upheld",
  "antoine": "Deck names must wrap at accessibility sizes.",
  "source": "reviews/2026-11-02.json"
}
```

`verdict` is `upheld` (the issue was right), `overruled` (it was not an issue, or the severity was wrong; `antoine` says which) or `missed` (a problem the critic did not report; `critic_said` is null).
