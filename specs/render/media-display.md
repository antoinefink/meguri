# Media names in displayed card HTML

Spec: render/media-display
Requirements: R530 (consistent with R988)
Anki: 26.09.3
Scenarios: conformance/scenarios/render/media-escape/

How the local media names in a card's HTML are percent-encoded before the HTML goes to a card web view, so that names with spaces, `#` or quotes load (R530). The encoding itself is `text/media-references#M6`; this file says what it applies to, when, and what it leaves alone. Outputs equal the oracle's string for string.

## D1. What is encoded, and when

Each card side that Meguri shows in a card web view is encoded as one text, with `text/media-references#M6`, before anything else is added to it:

| View | Side texts encoded |
|---|---|
| Review | the question and the answer (`render/type-answer#A3` step 2), including an answer rendered ahead of its reveal (U176) |
| Template preview (U71) | the side shown |
| Browse preview (U84, U22x) | the side shown, after its type-in markers are removed (`render/type-answer#A11.2`) |

- The text encoded is the side text of `render/type-answer#A1`: the `<style>` element, the side's HTML with play markers, type-in markers, and on the answer the question's HTML that `{{FrontSide}}` inserted. It is encoded once, as a whole: names written in the template (`<img src="_logo 2.png">`), names inserted from fields, names built from both (`<img src="{{File}}">`), and names repeated on the answer through `{{FrontSide}}` are all encoded, and none twice.
- Encoding comes after rendering and before the play buttons (U62's display transform), the type-in box and comparison (`render/type-answer`), and anything the page does. None of those adds a media tag, so none of them is encoded.
- Stored data never changes: fields keep raw names (R978); the type-in comparison reads the field as stored (`render/type-answer#A4.2`); AV tag lists keep the names as written (U62), and the player resolves them as file names (U67).

## D2. What changes in a side

Exactly M6 of `text/media-references`, summarized here; M2 to M6 there are normative.

- Only media tags change: `img`, `audio`, `video`, `object` and `source` elements with a `src=` or `data=` attribute (`data-src=` included), any case (M2).
- Remote names (`http://` and `https://`, any case) are left as they are (M5). So are sound tags, `[anki:play:…]` markers and text outside tags.
- In a local name, the ASCII controls, U+007F, space, `"`, `<`, `>`, `` ` `` and `#` become `%` and two upper-case hex digits. Everything else stays: `%`, `'`, `&`, `?`, `+`, `/` and every non-ASCII character. Entities are not decoded, so `a&quot;b.jpg` stays as written.
- Every copy of the name inside the tag changes with it (`<img title="a b" src="a b">` → `<img title="a%20b" src="a%20b">`).

## D3. Examples

Recorded from the oracle (`render/media-escape/strings`, `render/media-escape/cards`).

| In the side | Shown |
|---|---|
| `<img src="my file#1.jpg">` | `<img src="my%20file%231.jpg">` |
| `<img src="日本.png">` | unchanged |
| `<img src="a&quot;b.jpg">` | unchanged |
| `<img src="https://example.org/a b.png">` | unchanged |
| `<img src='single quoted.png'>` | `<img src='single%20quoted.png'>` |
| `<audio src="clip one.mp3"></audio><video src="film #2.mp4"></video>` | `<audio src="clip%20one.mp3"></audio><video src="film%20%232.mp4"></video>` |
| `<video><source src="a b.webm"></video>` | `<video><source src="a%20b.webm"></video>` |
| `<object data="plan v2.svg"></object>` | `<object data="plan%20v2.svg"></object>` |
| `<img data-src="lazy one.png">` | `<img data-src="lazy%20one.png">` |
| `[sound:voice clip.mp3]` | unchanged |
| `<img src="100% sure.png">` | `<img src="100%%20sure.png">` |
| `<img src="tab` TAB `here.png">` | `<img src="tab%09here.png">` |
| `<IMG SRC="UPPER CASE.PNG">` | `<IMG SRC="UPPER%20CASE.PNG">` |

A whole card, template `{{Word}}<br>{{Image}}<img src="{{File}}"><img src="_shared logo.png">{{Audio}}` with File `cat photo 3.jpg` and Audio `[sound:cat sound.mp3]<audio src="meow 2.ogg" controls></audio>`, shows `…<img src="cat%20photo%203.jpg"><img src="_shared%20logo.png">[anki:play:q:0]<audio src="meow%202.ogg" controls></audio>`; its answer, `{{FrontSide}}<hr id=answer><img src="https://example.org/remote pic.png">`, repeats those names encoded once and leaves the remote name alone.

## D4. Loading encoded names

The card web view resolves an encoded name to the media file of the decoded name: `my%20file%231.jpg` loads `my file#1.jpg` (U67's media scheme handler, R988's decoding direction is `text/media-references#M7`). Names that M6 leaves alone (`日本.png`, `a&quot;b.jpg`, which the HTML parser reads as `a"b.jpg`) load as the browser reads them.

## D5. Scenarios

| Scenario | Covers |
|---|---|
| `render/media-escape/strings` | D2 and D3 through the `text` observation's `iri_encode` (the plan's examples, every media tag kind, remote, sound and entity cases) |
| `render/media-escape/cards` | D1: whole review sides through `review_html` (`specs/conformance/type-answer-ops.md` K2): template, field and FrontSide names, remote names, sound tags, the type-in box and comparison untouched |
| `render/image-occlusion/render/cards` | an image-occlusion card's image name encoded beside its shape elements |

The many edge cases of the encoding itself (M2 matching, M8 round trips) are `conformance/scenarios/text/refs/iri` and the text corpus (U20).
