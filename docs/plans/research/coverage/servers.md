# Coverage note: servers


Coverage map (unit → R-IDs):
- U135: R1663, R1665, R1666, R1668, R1670, R1672 (status model), R1673–R1678, R1789 (write path), R1790 (read pool), R1949; AE78.
- U136: R1799, R1800 (harness), R1801, R1945, R1948, R1949 (runner hook).
- U137: R1680–R1690, R1672 (extension state).
- U138: R1692, R1710–R1720, R1718, R1947; AE71, AE87.
- U139: R1691–R1707, R1729, R1730, R1797 (Migaku branch); AE72, AE73.
- U140: R1721–R1728 (core), R1731–R1733; AE74, AE73 (Retry).
- U141: R1734–R1753, R1775, R1779–R1781, R1790 (atomicity), R1797; AE75, AE76, AE77.
- U142: R1754 (minus sync round trip), R1755–R1760, R1763–R1768, R1774, R1776–R1778, R1789 (preservation tests), R1945 (client scenarios).
- U143: R1761, R1762, R1769–R1773; AE79 (core).
- U144: R1663, R1664, R1665, R1666, R1667, R1669, R1671, R1672, R1679, R1697 (transcoder), R1742/R1748 (UI), R1749–R1750 (alert), R1770, R1772 (grants), R1779–R1781 (UI), R1787 (setting).
- U145: R1709, R1710–R1715 (UI), R1720 (UI), R1721–R1730 (UI); AE71, AE74 (UI).
- U146: R1754 (sync), R1782–R1788, R1733 (searchNote navigation).
- U147: R1669, R1670, R1671, R1679, R1708, R1791, R1792–R1796, R1797, R1798, R1946; AE66 (servers part), AE79, AE80; F3.
- R1946 is enforced by U137/U139 scenarios (in-process) and U147 (app-level).

R-IDs in range whose implementation is owned by another area (this area only verifies them in U147):
- R1708 (review counts/progress update without re-render): Review UI (U175) + queue rebuild contract (U41/U43, keep pre-renders by cardId+stateToken).
- R1792 (UI within 1 s; Browse in-place refresh and "Results changed — Refresh"): change stream U24, Home U152, Browse U210, heatmap stats U95/U225.
- R1793 (editor Reload / Keep mine): Notes UI editor U190.
- R1794 (current card re-render or advance): Review U175.
- R1795, R1796 (API ops not undoable; truncation): undo manager U25 (truncate at deepest intersecting entry, disabled reason ChangedExternally, KTD7).
- R1801 similarity gate mechanism: U2 (this area extends its corpus in U136).
- R1671 entitlement presence: U7 (macOS entitlements); this area verifies the no-prompt behavior.
- R1667 window-close behavior mechanism: U150 Mac app delegate (spike S4); this area supplies `integrationsEnabled` and the login item.

Cross-area contracts assumed:
- U24/C1: op `source=LocalApi`; lifecycle waiter with timeout (C7); read-only connection pool usable from tokio blocking threads; change events carry `source`.
- U25: composite op (create + delete) recorded as one undo entry (U140 moves); API ops never pushed to undo; truncation rule per KTD7.
- U27: note-add with `source=LocalApi` never writes `curModel`, `_nt_*_lastDeck`, `_deck_*_lastNotetype` (Flow 4 default; deviation from R801 for API adds; U195 must keep R801 for UI adds).
- U31/C9: media staging inside an op with rollback, content-identical reuse, conflict rename returning the stored name, trash move; a scan entry point for externally added files.
- U60: render question/answer HTML for a card id (cardsInfo, empty-front check) without the card runtime.
- U45 (through U43): next-state interval labels for `nextReviews`; U48: Forget and Set Due Date ops with `days` grammar.
- U41 deck-list counts; U95 day boundaries; U80 search entry point with injectable current deck and ascending-id result ordering.
- U6 CoreBridge: servers FFI only in the macOS slice; host-event callback interface and `resolve_interaction`; transcoder callback (synchronous, invoked off main).
- U150: `AppNavigation` intents (show Browse with query/sort/selection/editor, present Add with prefill), sidebar item registration with badge, `Commands` composition, delegate keep-alive flag; `UiContext` (selected deck, Browse visible) pushed to the core.
- U151: sync coordinator entry point returning success or an error string for the `sync` action.
- U195: Add panel prefill API, unsaved-draft query, `closeAfterAdding`.
- U210/U84: Browse query/sort by Anki column id, selection read/write, open editor.
- U66/U67: card pages must not load under an origin that either server's default policy allows (no `http(s)://localhost`/`127.0.0.1`, no extension schemes, no `null` reliance) (R1679).
- U2/U3: oracle image variant with aqt + AnkiConnect under `oracle/ankiconnect/`; similarity corpus extension.

Conflicts and open questions:
- R822 vs R1793: R822 says a local-server change saves pending edits then reloads; R1793 says keep unsaved edits and show "Reload / Keep mine". This plan follows R1793 for `source=LocalApi` (spec-flow gap 8); the Editor area (U190) must apply it.
- R1659 vs R1797: R1659 says local-server requests during a sync wait and are never lost; R1797 fails AnkiConnect requests after 30 s. Under KTD6 a normal sync holds no long write lock, so waiting applies only to `ExclusiveBusy`; AnkiConnect then fails after 30 s (the client sees the error, nothing is silently lost) and Migaku uses the durable pending queue.
- KTD13 says "~200 MB" body limit; R1678 sets 256 MB (AnkiConnect) and 128 MB (Migaku). Units follow R1678.
- R1706 pending reply `{"success": true, "created": false}`: the requirements doc defers verifying that extension 1.31 treats it as success. Kept behind one switch in U139; verification against the real extension belongs in U147's manual checklist, with fallback HTTP 400 "Meguri is busy".
- R1674 message text says "or change the port" also for Migaku, whose port is fixed (R1675); UI may need a Migaku-specific variant. Left to the Product Contract owner.
- R1770: behavior of `getMediaDirPath` before the user grants a folder is unspecified; U143 returns an error naming the setting (returning the in-container path would trigger macOS "App Data" consent prompts for external tools).
- R1801/R1945: the AnkiConnect 2025-11-09 build may not match any public upstream commit; then the recorder image needs the add-on tarball from `local-data/` (local-only build). aqt offscreen under libfaketime in Docker is unproven (spike in U136; host-recording fallback).
- R1947/R1718 replay depends on private data, so it can only be a local gate (never hosted CI) run by a role that may read `local-data/`; implementers use a synthetic corpus.
- R1697 needs an AAC encoder and loudness measurement; done in Swift (AVFoundation) via a core callback, so the normalization path is not testable in Docker beyond a stub.
- App Store naming of "Migaku" (trademark) unresolved; strings centralized for a one-key rename.
- New crate dependency `md-5` (rust-stack §5) is needed for `skipHash`; license MIT/Apache (allowed).
