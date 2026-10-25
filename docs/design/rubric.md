# Meguri design rubric

Version 2 (U9, R2001). The critic grades screenshots and contact sheets against these rules and cites them as `rubric v2 <rule id>` (R1983). Rule ids are stable: a rule that changes meaning gets a new id, and a retired rule stays listed as retired. Bump the version when a rule is added, changed or retired, and note it under Changes.

## Owners

Each rule has an owner. Where the owner is a tool, the tool's result is the source of truth (R1986): the critic may point at such a problem, but its issue stands only when the tool fails too, or when the case lies outside what the tool measures (named in the rule). Critic-owned rules are judged from the screenshots.

| Owner | What it is |
|---|---|
| `lint:<id>` | `tools/mg lint-ui` rule `<id>` (`app/.swiftlint.yml`, `tools/lint-ui/`); fails the build before any other gate |
| `test:ContrastTests`, `test:TokenCoverageTests` | DesignSystem tests (`tools/mg swift-test --package MeguriKit`) |
| `audit` | the system accessibility audit at default and largest text sizes, light and dark (U10, U253) |
| `touch-targets` | hierarchy frame check: 44×44 pt on iPhone and iPad, 28×28 pt on the Mac (U10) |
| `hierarchy` | `hierarchy-check` over UI hierarchy exports: identifiers, overlapping and off-window text (U8) |
| `contact-sheet` | jump-cut and freeze detection on recorded transitions (U8, U252) |
| `states` | `StateCoverageTests` and inventory coverage: every listed state has a preview and a snapshot (U10, U252) |
| `critic` | the fresh-context critic (U10, U254) |

## Severity

- **High**: the screen is wrong for its user: text clipped, overlapping or unreadable; a control unreachable, unlabeled or too small; glass on content; a custom replacement for a system component; layout broken at a supported size, text size or appearance; motion that pops or jumps; anything that resembles Activity rings. Keeps the screen out of "verified" (R1985).
- **Medium**: the screen works but falls short of the brief: weak hierarchy, uneven alignment or spacing rhythm, an icon at the wrong weight, a missing empty or error state, a platform idiom ignored. Becomes a task.
- **Low**: taste. Recorded for the weekly gallery; never blocks a merge (R1985).

## Rules

### Platform and structure

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `platform-system-components` | System components (lists, forms, split views, tab bars, toolbars, tables, search fields, sheets, inspectors, menus) are used wherever they fit (R1995). | critic | high |
| `platform-native-defaults` | Preserve each platform's default typography, colors, control sizing, spacing and interaction. Overrides need a documented functional, compatibility or verified accessibility reason; tokens alone are not a reason. Compare the exact native build with unmodified system controls, including relevant disabled, selected and focus states. Cosmetic exceptions need Antoine's approval (`native-interface-contract.md`, 2026-10-25). | critic | high |
| `platform-no-custom-bars` | No custom tab bars, custom navigation bars or hamburger menus (R1995). | `lint:custom-tab-bar`, `lint:custom-navigation-bar`, `lint:hamburger-menu`; critic for look-alikes built under other names | high |
| `platform-idioms` | Each platform's idioms hold: Mac menus, shortcuts, hover and resizable windows; iPad sidebar, inspector, pointer and keyboard; iPhone thumb reach and navigation stacks. | critic | medium |
| `platform-adaptivity` | The layout works at every supported width: small and large iPhone, iPad compact and regular, resizable iPhone windows, Mac windows from minimum to large. | critic | high |
| `components-native-or-justified-shared` | Use native components directly. Register justified custom reusable components in the gallery; do not add wrappers merely to satisfy gallery conventions (R1999, superseded 2026-10-25). | critic | medium |

### Liquid Glass and content

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `glass-control-layer-only` | Glass and materials appear only on the navigation and control layer: sidebar, tab bar, toolbars, answer button bar, floating controls (R1996). | `lint:glass-outside-chrome` | high |
| `glass-no-stacking` | Glass is never placed on glass; grouped controls share one glass container. | critic | high |
| `glass-legibility` | Content under glass stays legible while scrolling (scroll edge effects, no busy content behind labels), with Reduce Transparency and both glass looks. | critic | medium |
| `card-content-untouched` | The card face shows exactly what the card's templates and base styles produce; Meguri's chrome never restyles it (R2000). | critic, rendering tests (U66, U67) | high |
| `depth-no-content-shadows` | Content carries no shadows; depth comes from the control layer. | `lint:content-shadow` | medium |

### Colour

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `color-native-or-justified-token` | Preserve system semantic colors and control appearance. Justified custom colors use tokens with light, dark and increased-contrast variants (R1992, R1994, superseded 2026-10-25). | critic; `lint:raw-color`, `test:TokenCoverageTests` for custom values only | medium |
| `color-contrast` | Text meets 4.5:1 and large text and non-text UI 3:1 against what it is drawn on, in light, dark and increased contrast (R1979). | `test:ContrastTests` for token pairs; `audit` for text over images, glass and system materials | high |
| `color-not-only-cue` | Colour is never the only cue: counts keep column titles or labels, ratings keep their names, states keep symbols (R187, R1982). | critic, VoiceOver scripts (U253) | high |
| `color-meaning` | A token means the same everywhere: New blue, Learn red, Due green, zero dimmed (R32); Again red, Good green, Hard and Easy neutral (R355). | critic | medium |

### Text

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `type-system-styles` | Text uses system text styles that scale with Dynamic Type; no fixed sizes or custom fonts (R1993). | `lint:fixed-font-size` | medium |
| `text-truncation` | No text is clipped or cut off at any supported text size, width or language. Truncation is allowed only where the screen's spec allows it, with the full text in the accessibility label (and a tooltip on the Mac). | `audit` (`textClipped`), truncation check (U253); critic for what the audit cannot see | high |
| `text-overlap` | Text never overlaps other text or controls, and never leaves its window. | `hierarchy`; critic | high |
| `type-hierarchy` | One clear reading order per screen: a title, then content, then secondary detail; no more text styles than the content needs. | critic | medium |
| `copy-plain` | UI text is short, plain and specific to the action ("Delete Deck", not "OK"). | critic | low |

### Layout

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `layout-native-spacing` | Preserve implicit system spacing, padding and insets. Justified custom spacing uses Spacing tokens; do not add modifiers merely to use tokens. | critic; `lint:numeric-spacing` for custom values only | medium |
| `layout-native-shapes` | Preserve standard control shapes and corner radii. Justified custom shapes use Radius tokens or concentric system shapes. | critic; `lint:numeric-radius` for custom values only | medium |
| `layout-alignment` | Elements share alignment lines and the system's layout margins and readable width; spacing rhythm is consistent. | critic | medium |
| `layout-states` | Every screen state the inventory lists exists and is designed: empty, typical, overflow, error, loading (R1970). | `states`; critic for design quality | medium |

### Accessibility and input

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `a11y-audit` | The system accessibility audit passes at default and the largest accessibility size, light and dark; a suppression names one element, one audit type and a reason (R1977). | `audit` | high |
| `a11y-touch-targets` | Touch targets are at least 44×44 pt on iPhone and iPad and 28×28 pt on the Mac (R1980). | `touch-targets` | high |
| `a11y-identifiers` | Every interactive control and list row has an `area.screen.element[.id]` identifier (R1964). | `hierarchy` | medium |
| `a11y-alternatives` | Invisible tap zones and gestures have named alternatives for VoiceOver, Switch Control, Voice Control and Full Keyboard Access (R1981). | critic, VoiceOver scripts (U253) | high |

### Motion

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `motion-native-or-justified-token` | Preserve automatic system motion. Justified explicit animations use Motion tokens with Reduce Motion alternatives. | critic; `lint:raw-motion`, `test:TokenCoverageTests` for custom motion only | medium |
| `motion-continuity` | Transitions show continuity: no jump cuts, layout pops or partial frames; with Reduce Motion a crossfade replaces movement (R1976). | `contact-sheet`; critic | high |

### Icons

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `icon-sf-symbols` | Icons are SF Symbols; no other icon family (R1998). | `lint:non-sf-symbol-icon` | medium |
| `icon-weight-match` | A symbol's weight and scale match the text beside it (R1998). | critic | medium |

### Stats visuals

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `stats-no-activity-rings` | Streak, goal and heatmap visuals do not resemble Apple's Activity rings: no concentric ring progress (R1997). | critic | high |

### Retired rules

| Id | Rule | Owner | Usually |
|---|---|---|---|
| `color-tokens-only` | Retired in v2; replaced by `color-native-or-justified-token`. | retired | retired |
| `layout-spacing-tokens` | Retired in v2; replaced by `layout-native-spacing`. | retired | retired |
| `layout-radius-tokens` | Retired in v2; replaced by `layout-native-shapes`. | retired | retired |
| `motion-tokens` | Retired in v2; replaced by `motion-native-or-justified-token`. | retired | retired |
| `components-from-gallery` | Retired in v2; replaced by `components-native-or-justified-shared`. | retired | retired |

## Lint rule index

The `lint-ui` rule ids, for reading gate output (`app/.swiftlint.yml` holds the patterns):

| Lint rule | Rubric rule |
|---|---|
| `raw-color` | `color-native-or-justified-token` |
| `fixed-font-size` | `type-system-styles` |
| `numeric-spacing` | `layout-native-spacing` |
| `numeric-radius` | `layout-native-shapes` |
| `raw-motion` | `motion-native-or-justified-token` |
| `content-shadow` | `depth-no-content-shadows` |
| `glass-outside-chrome` | `glass-control-layer-only` |
| `custom-tab-bar`, `custom-navigation-bar`, `hamburger-menu` | `platform-no-custom-bars` |
| `non-sf-symbol-icon` | `icon-sf-symbols` |
| `inline-disable`, `allowlist-invalid` | none: a misused exemption, fixed in place |

## Changes

- v2 (2026-10-25, Antoine): require native platform defaults, document exceptions and replace token-only rules. Existing lint patterns constrain explicit custom values; they do not enforce native appearance.
- v1 (U9): first version.
