# Native deck-list preview

Starting commit: ece649a31d6d9bbe983cd83148ff98044a68d581. This bounded UI pass is not a completed plan unit. Ongoing U107 work is preserved in a named local stash and remains unaccepted.

The Decks root now uses a native List outside the surrounding ScrollView. Standard-weight names lead; nonzero labeled counts use secondary text. All counts remain accessible. Native disclosure exposes nested decks; parent and child actions still use AppNavigation.openDeckPage. Selected-deck detail and sidebar are unchanged.

Implementation session: s-20261025T152526Z-abb26c6a; filtered export 20261025T152619Z-97669. Independent source review found no P0–P2 findings. Fixer session s-20261025T153636Z-ca8089d8 removed an unnecessary comment flagged by privacy checks and repeated ordinary-deck icons; filtered export 20261025T153711Z-8165. Independent delta review was clear. Final focused UI lint passed (20261025T154437Z-15799); source similarity passed (20261025T154434Z-15610).

The first native host Xcode MCP build passed in 37.189 seconds with zero errors. The integrator inspected that exact running Debug app and confirmed native list rows and quieter typography. The final refinement passed the signed native host build 20261025T155349Z-24602, and the exact product was inspected in the running app. The original privacy failure is retained at build/mg/private/20261025T153309Z-3552; repaired files pass 20261025T154100Z-12247. Xcode-generated unrelated catalog edits were preserved separately and removed from this change.

The final render exposed a grouping issue: Previous Session also grouped deck rows. Filtered fixer 20261025T160607Z-41767 puts deck choices first in their own section and history last, with matching top-scroll order. Independent follow-up source review found no P0–P2 findings. Signed native build 20261025T160930Z-45369 passed; the app was reopened from that exact product. Nonempty-history and nested scroll renders remain pending.

Pending: parent/disclosure interactions, nested/offscreen and leading history/error scroll restoration, platform/layout checks and required integration gates. The temporary host macOS-first authorization defers VM/mobile iteration; it does not pass those checks. User media feedback takes priority before further design iteration. No design or milestone acceptance claimed.
