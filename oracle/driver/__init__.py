# SPDX-License-Identifier: AGPL-3.0-or-later
"""Meguri oracle driver: runs neutral scenarios on the pinned `anki` package.

This package is the only Meguri-side code that imports `anki` (R1873). It speaks
the JSON-lines protocol described by conformance/schema/protocol.json and never
emits Anki's localized message text (R1887).

Modules:
  worker     protocol loop of one long-lived worker process (python -m driver)
  session    one scenario's collection, clock and logical ids
  registry   op, observation and setup-section registration (per-area modules)
  ops/       ops by area; observe/ observations by area
  harness    worker pool, golden recording, live comparison and bump reports
"""

PROTOCOL_VERSION = 1
