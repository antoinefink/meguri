# Media file name transform

Spec: text/media-filenames
Requirements: R979
Anki: 26.09.3
Scenarios: conformance/scenarios/text/filenames/, conformance/scenarios/text/corpus/filenames-01.json

The pure transform a desired media file name goes through before the media store adds a file. The store's own rules (F4) build on it.

## F1. Steps

Apply in this order:

1. **NFC.** Normalize to NFC (`unicode-normalization` 0.1.24, Unicode 16.0.0, KTD2).
2. **Remove characters.** Delete every character that is
   - one of `[ ] < > : " / ? * ^ \ |`;
   - an ASCII control character, U+0000–U+001F or U+007F (so TAB goes too);
   - unassigned in **Unicode 10.0.0**: General_Category Cn in that version's character database, which includes the noncharacters (U+FDD0–U+FDEF and every U+xxFFFE and U+xxFFFF). Characters added in Unicode 11.0 or later are therefore removed (U+1F970 and U+1FAE9 go, U+1F600 stays). Private-use characters (Co) and format characters (Cf) are kept.
3. **No-break space.** Replace each U+00A0 with U+0020. Other space characters (U+202F, U+3000, ...) stay.
4. **Windows device names.** If the name starts with `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9` or `LPT1`–`LPT9` (ASCII case-insensitive) and that word is followed by `.` or by the end of the name, insert `_` right after the word: `con.jpg` → `con_.jpg`, `con` → `con_`, `nul.tar.gz` → `nul_.tar.gz`, `CON.jpg` → `CON_.jpg`. `console.txt`, `com0`, `com10.txt` and `lpt10.png` are unchanged.
5. **Trailing space or dot.** If the name ends with U+0020 or `.`, append one `_`: `test.` → `test._`, `test ` → `test _`, `con.` → `con_._`, `..` → `.._`.
6. **Length.** If the UTF-8 form is longer than 120 bytes:
   1. Split at the last `.`: the extension is what follows it, the stem what precedes it. With no `.`, the stem is the whole name and the extension is empty.
   2. Cut the extension to at most 10 bytes, then the stem to at most 120 − (extension bytes) − 2 bytes. Each cut keeps the longest prefix that ends on a character boundary within the limit.
   3. Join as stem + `.` + extension, or the stem alone when the extension is empty.
   4. If the result ends with U+0020 or `.`, append one `_`.

## F2. Properties

- The result is at most 120 bytes.
- Characters are not normalized again after step 2: removing a character can leave a decomposed sequence (`e?` + U+0301 + `.png` → `e` + U+0301 + `.png`), and running the transform again would compose it.
- The result can be empty (`???`). The transform returns the empty string; the media store must refuse to add a file under it (U31).
- Leading dots are kept (`.png` → `.png`, `.` → `._`).

## F3. Examples

| Input | Result |
|---|---|
| `a:b?.png` | `ab.png` |
| `[x]<y>"z"/w*^\|.png` | `xyzw.png` |
| `a` U+0001 `b` U+001F `c` U+007F `.png` | `abc.png` |
| `tab` TAB `name.png` | `tabname.png` |
| `a` U+00A0 `b.png` | `a b.png` |
| `e` U+0301 `.png` | `é.png` (U+00E9) |
| U+1100 U+1161 `.png` | U+AC00 `.png` |
| U+0378 `.png`, U+FDD0 `.png`, U+FFFF `.png`, U+08BE `.png` | `.png` |
| U+E000 `.png`, U+E0001 `.png`, U+0860 `.png` | unchanged |
| `x` × 196 + `.jpg` (200 bytes) | `x` × 115 + `.jpg` |
| `y` × 200 | `y` × 118 |
| `日` × 70 + `.png` | `日` × 38 + `.png` (118 bytes) |
| `q` × 116 + `.jpg` (120 bytes) | unchanged |
| `a` × 100 + `.` + `b` × 30 | `a` × 100 + `.` + `b` × 10 |
| `m` × 120 + `.` + `é` × 6 | `m` × 108 + `.` + `é` × 5 |
| `n` × 120 + `.` + U+1F600 × 3 | `n` × 110 + `.` + U+1F600 × 2 |
| `w` × 115 + `.abcdefghi jkl` | `w` × 108 + `.abcdefghi _` |
| `t` × 117 + ` ` + `t` × 10 | `t` × 117 + ` _` |
| `z.abcdefghijklmnop` | unchanged (short names keep long extensions) |

## F4. Not part of the transform

These are media-store behavior (U31), built on F1: reusing an existing file with identical content (R980), lowercasing new names (R981: `Photo.JPG` is stored as `photo.jpg`), the `<stem>-<40-hex SHA-1>.<ext>` name for a conflicting file (R982), and ignored names (R985). The oracle observes this transform through its media store, which also lowercases, so the `media_filename` observation accepts only inputs that lowercasing leaves unchanged; the upper-case rows of F1 step 4 are covered by this spec, not by goldens.

Deciding whether a name already on disk is in normalized form (Check Media renames files that are not) is U130's, on top of this transform.

## F5. Scenarios

- `filenames/normalize`: every step and the examples above.
- `corpus/filenames-01`: 2,000 distinct generated names (device prefixes, removed and kept characters, multi-byte truncation).
- Observation function (`specs/conformance/observations.md` O9): `media_filename`.
