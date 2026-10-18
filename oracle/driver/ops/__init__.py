# SPDX-License-Identifier: AGPL-3.0-or-later
"""Ops and setup sections, one module per area. Importing registers them."""

from . import (  # noqa: F401
    browser,
    clock,
    collection,
    deck_options,
    decks,
    handoff,
    maintenance,
    media,
    model,
    notes,
    notetypes,
    package,
    presets,
    preservation,
    sched,
    storage,
    storage_open,
    sync_client,
    sync_server,
    tags,
    undo,
)
