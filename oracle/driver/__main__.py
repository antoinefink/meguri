# SPDX-License-Identifier: AGPL-3.0-or-later
"""python -m driver: one oracle worker on stdin/stdout (start it via entrypoint.sh)."""

import sys

from .worker import main

sys.exit(main())
