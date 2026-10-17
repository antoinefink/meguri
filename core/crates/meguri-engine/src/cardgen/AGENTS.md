# cardgen/

- G6's fallback ignores template 0's deck override. Regeneration excludes cardless notes and never applies G6.
- A sibling's position is chosen by lowest card id among type-New cards, independent of queue; it bypasses both counter and random insertion.
- `regenerate::plan` accepts U34's in-memory type. Its caller owns type-draft preconditions; `commit_in` re-selects the note population and re-derives against live rows, including notes added since planning. `regenerate` reads the stored type under executor preconditions; `start` runs it on the job pool.
