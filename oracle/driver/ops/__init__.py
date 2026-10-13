# SPDX-License-Identifier: AGPL-3.0-or-later
"""Ops and setup sections, one module per area. Importing registers them."""

from . import clock, collection, decks, handoff, notes, notetypes, sched, storage  # noqa: F401
