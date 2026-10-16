# SPDX-License-Identifier: AGPL-3.0-or-later
"""The names and bounds of `tools/mg oracle time`
(specs/conformance/timed-operations.md T1, T6), without importing anki: the
harness checks --op and --repeat with them before any worker starts.
"""

from __future__ import annotations

# T6, in catalogue order.
OPERATIONS = (
    "collection.open",
    "decks.counts",
    "queue.build",
    "review.answer",
    "search.deck",
    "search.state",
    "search.tag",
    "search.text",
    "search.regex",
    "search.prop",
    "search.rated",
    "search.combined",
    "browser.sort_field",
    "browser.sort_due",
    "stats.graphs",
    "maintenance.check_db",
    "package.apkg_export",
    "package.colpkg_export",
    "package.apkg_import",
    "sync.full_upload",
    "sync.full_download",
    "fsrs.recompute_memory_states",
)
WARMUP = 1
REPEAT_DEFAULT = 5
REPEAT_MIN, REPEAT_MAX = 1, 100


def in_catalogue_order(names) -> list[str]:
    """The named operations once each, in catalogue order (T1)."""
    wanted = set(names)
    return [name for name in OPERATIONS if name in wanted]
