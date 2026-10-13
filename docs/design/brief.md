# Meguri design brief

Version 1 (U9, R2001). This file says what Meguri should feel like and why. `rubric.md` turns it into rules with ids, `screen-inventory.yaml` lists the screens, `decisions.md` logs Antoine's picks and vetoes, and the component gallery (`designsystem/gallery` route) shows the parts screens are built from.

## Feel

Meguri feels like an app Apple would ship on each platform: calm, quick and quiet. The card is the hero. Everything around it recedes until it is needed, and nothing makes Antoine wait: the next card is on screen the moment he answers.

## Principles

1. **Content first, controls above it.** Card faces, lists and charts are content. Navigation and controls sit on a separate layer in Liquid Glass: the sidebar, tab bar, toolbars, the answer button bar and floating controls (R1996). Content never gets glass, and glass is never stacked on glass.
2. **System components first.** Lists, forms, split views, tab bars, toolbars, tables, search fields, sheets, inspectors and menus are used wherever they fit (R1995). A custom view is built only when no system component does the job, and then from the gallery's components.
3. **One vocabulary.** Colours, spacing, corner radii and motion come from named tokens; text uses system text styles; icons are SF Symbols (R1992, R1993, R1998). The same token means the same thing everywhere: New is blue, Learn red, Due green (R32), Again red and Good green (R355).
4. **Accessible by construction.** Every screen works at the largest Dynamic Type sizes, in dark mode and with Increase Contrast, Reduce Transparency, Reduce Motion, VoiceOver, Voice Control, Switch Control and Full Keyboard Access (R1977–R1982). Colour is never the only cue. Text wraps before it truncates.
5. **Motion explains change.** Animations show where something came from or went; they are short, and with Reduce Motion they become crossfades or cuts (R1976, R1992). Nothing animates on the review path that would delay the next card.
6. **Each platform on its own terms.** The Mac has menus, keyboard shortcuts, pointer hover and resizable windows; iPad has the sidebar, inspector, pointer and keyboard; iPhone puts the frequent actions within thumb reach. One design, adapted, never a phone layout stretched to a window.
7. **Quiet status.** Routine confirmations are short toasts; errors appear where they happened, with a way forward. Modal alerts are kept for decisions that cannot wait.
8. **Anki's behaviour, Meguri's own interface.** Meguri does what Anki does, in its own words and layout (R1959). Card templates render exactly as Anki renders them.

## Anti-goals

- **Activity rings.** Streak, goal and heatmap visuals must not resemble Apple's Activity rings: no concentric circular progress rings, no ring-closing metaphor (R1997).
- **Restyled cards.** Meguri's chrome never restyles card content beyond the Anki-compatible base styles that templates expect (R2000): no app fonts, colours, padding or backgrounds pushed into the card page.
- **Custom chrome.** No custom tab bars, custom navigation bars or hamburger menus (R1995).
- **Glass on content.** No glass or material behind card faces, list rows or charts; no glass on glass (R1996).
- **Off-system styling.** No raw colours, hex values, fixed font sizes, ad-hoc spacing, radii or animations, no shadows on content, no icon families other than SF Symbols (R1994, R1998).
- **Decoration that competes with study.** Nothing animates, glows or badges itself on the review screen beyond the card and its controls.

## Signature screens

Home and deck list, deck page, review (question and answer sides), browser, editor, deck options and stats (R1972 and the design-quality Success Criterion). Each gets about three design variants for Antoine to pick from (R1988), the full snapshot matrix and his sign-off; picks go in `decisions.md`.

## How a screen is judged

Deterministic checks come first and are the source of truth for what they measure (R1986): the UI lint gate (`tools/mg lint-ui`), the contrast and token tests, the accessibility audit, hit-target and hierarchy checks. The critic then grades screenshots against `rubric.md` and the references in `references/` (R1983). High-severity issues keep a screen out of "verified"; taste alone never blocks a merge (R1985).
