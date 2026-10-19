# type_answer/

- A6 uses two Unicode versions: 16 for normalization/nc, 10 for token-leading mark isolation. A9's simple folding uses separately pinned ICU4X 2.0 data; full folding changes token positions.
- `unicode10_marks.rs` contains only Mn/Mc/Me intervals from neutral `specs/render/unicode10-marks.json` (official UnicodeData 10; Unicode-3.0 notice at `specs/render/unicode-data-LICENSE.txt`). Keep its exhaustive reference test; never derive this set from current categories or character age.
- Parse the completed question replacement, not the font alone: a font-ending backslash precedes punctuation in that template. Expand captures per original match without rescanning inserted text. Answer font insertion is literal.
