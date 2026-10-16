# cloze/

- Behavior is `specs/render/cloze.md`, which follows the oracle (C13): parsing ignores MathJax, so a `}}` inside `\(…\)` closes the cloze, and type-in answers collapse only when all are identical. R973 and R977 say the same since plan amendments A1 and A2 (`docs/process/review-log/U61.md`); MathJax-aware parsing would be a deviation and changes card generation. Only `cloze:` runs the C8 MathJax pass, so span balance holds only where C12 says.
- At most 10 clozes are open (C4), so the tree and its walks recurse. Front output still grows about 50,000-fold for a character ten active levels deep (C12): never cap it (that changes the output), and keep fuzz inputs at 1 KiB or less.
- `render.rs`'s `image_occlusion_element` is U64's hook (C11) and returns the empty string until then. Image-occlusion clozes are ordinary everywhere except `cloze:`.
