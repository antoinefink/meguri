# SPDX-License-Identifier: AGPL-3.0-or-later
"""python -m driver: one oracle worker on stdin/stdout (start it via entrypoint.sh)."""

import sys

from .errors import install_excepthook

install_excepthook()  # before anki is imported

from .worker import main  # noqa: E402

sys.exit(main())
