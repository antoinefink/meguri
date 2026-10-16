# cloze/

- Behavior is `specs/render/cloze.md`, which follows the oracle where the plan differs (C13): parsing ignores MathJax, so a `}}` inside `\(…\)` closes the cloze (R973's first clause waits for a product decision), and type-in answers collapse only when all are identical. Only `cloze:` runs the C8 MathJax pass.
- At most 10 clozes are open (C4), so the tree and its walks recurse. Front output still grows about 50,000-fold for a character ten active levels deep (C12): never cap it (that changes the output), and keep fuzz inputs at 1 KiB or less.
- `render.rs`'s `image_occlusion_element` is U64's hook (C11) and returns the empty string until then. Image-occlusion clozes are ordinary everywhere except `cloze:`.
