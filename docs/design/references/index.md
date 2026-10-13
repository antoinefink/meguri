# Reference screenshots

Version 1. The critic compares screens with these references (R1983); the rubric (`../rubric.md`) and the screen inventory (`../screen-inventory.yaml`) cite them by pattern id.

Antoine captures the screenshots on his own devices; agents never capture, download or generate them. Until a pattern has files, the critic works from the rubric alone for it.

- File names: `<pattern id>-<platform>[-<n>].png` in this folder, where platform is `iphone`, `ipad` or `mac` (for example `sidebar-mac-1.png`).
- Capture the current OS release, light appearance, default text size, with no personal data on screen. Add a dark capture when the pattern changes in dark mode.
- Screenshots of Apple's and other developers' apps are kept only while this repository is private. If it ever becomes public, they move out of git first.

| Pattern | What it shows | Where to capture | Platforms |
|---|---|---|---|
| `sidebar` | Source-list sidebar with sections, counts and selection | Mail, Notes, Files | ipad, mac |
| `tab-bar` | Tab bar with a search tab, minimized while scrolling | Music, Photos, App Store | iphone |
| `toolbar` | Glass toolbar items grouped, with a primary action | Notes, Mail, Finder | iphone, ipad, mac |
| `floating-controls` | Glass controls floating over content, merged into one shape | Maps, Camera, Photos viewer | iphone, ipad |
| `list-counts` | List rows with trailing counts and disclosure | Mail mailboxes, Reminders lists | iphone, ipad, mac |
| `grouped-form` | Settings-style grouped form with toggles, pickers and footers | Settings, System Settings | iphone, ipad, mac |
| `content-unavailable` | Empty and no-results states | Mail search without results, an empty Notes folder | iphone, mac |
| `table` | Sortable multi-column table with selection | Finder list view, Music songs | ipad, mac |
| `search-tokens` | Search field with tokens and suggestions | Mail search, Photos search | iphone, mac |
| `inspector` | Inspector panel beside content | Pages, Keynote, Freeform | ipad, mac |
| `sheet-form` | Sheet with a short form and Cancel/Done | Calendar new event, Reminders details | iphone, ipad, mac |
| `confirmation-dialog` | Destructive confirmation (action sheet on iPhone, popover or alert elsewhere) | Photos delete, Mail delete mailbox | iphone, ipad, mac |
| `charts` | Charts with a scope picker, labels and tables | Health trends, Screen Time, Weather details | iphone, ipad, mac |
| `calendar-grid` | Day grid with intensity shading (heatmap reference; never rings, R1997) | Health cycle calendar, Fitness history calendar | iphone, mac |
| `progress-header` | Thin progress indicator under a navigation bar | Safari page load, App Store download | iphone, mac |
| `editor` | Rich-text editing with a formatting toolbar and keyboard accessory | Notes, Mail compose | iphone, ipad, mac |
| `toast` | Short transient confirmation over content | Music "Added to Library", Notes moved | iphone, mac |
| `onboarding` | Welcome screen with a few feature rows and a continue button | Journal, Freeform first launch | iphone, ipad, mac |
| `widget` | Small and medium widgets with a grid or count | Fitness, Screen Time widgets | iphone, mac |
