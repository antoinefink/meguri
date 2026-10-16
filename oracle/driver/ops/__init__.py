# SPDX-License-Identifier: AGPL-3.0-or-later
"""Ops and setup sections, one module per area. Importing registers them."""

from . import (  # noqa: F401
    browser,
    clock,
    collection,
    decks,
    handoff,
    maintenance,
    model,
    notes,
    notetypes,
    package,
    sched,
    storage,
    storage_open,
    sync_client,
    sync_server,
    tags,
    undo,
)
