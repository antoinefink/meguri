# type_answer/

- A6 uses two Unicode versions: 16 for normalization/nc, 10 for token-leading mark isolation. A9's simple folding uses separately pinned ICU4X 2.0 data; full folding changes token positions.
- Parse the completed question replacement, not the font alone: a font-ending backslash precedes punctuation in that template. Expand captures per original match without rescanning inserted text. Answer font insertion is literal.
