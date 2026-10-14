# SPDX-License-Identifier: AGPL-3.0-or-later
"""Ops and setup sections, one module per area. Importing registers them."""

from . import clock, collection, decks, handoff, model, notes, notetypes, package, sched, storage, sync_client, sync_server  # noqa: F401
