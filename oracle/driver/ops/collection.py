# SPDX-License-Identifier: AGPL-3.0-or-later
"""Collection-wide settings.

setup.config: {<config key>: <JSON value>}   written with the collection's
config API in sorted key order (e.g. fsrs, loadBalancerEnabled, rollover).
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import setup_section


@setup_section("config", order=10)
def config(session, values):
    if not isinstance(values, dict):
        raise invalid("setup.config is a mapping of config keys to values")
    for key in sorted(values):
        session.col.set_config(key, values[key])
