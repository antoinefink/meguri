# SPDX-License-Identifier: AGPL-3.0-or-later
"""An add-on area loaded through ORACLE_EXTRA_MODULES (test_worker.ExtensionPoint).

`shout` also writes to stdout from Python and at the file-descriptor level, as
a misbehaving library would; the protocol stream must not see it.
"""

import os
import sys

from driver.registry import observation, op


@op("shout")
def shout(session, args):
    print("noise from print()")
    sys.stdout.flush()
    os.write(1, b"noise on fd 1\n")
    return str(args).upper()


@observation("echo")
def echo(session, args):
    return args


@op("unjsonable")
def unjsonable(session, args):
    """A result JSON cannot carry, as a buggy area module might return."""
    return {"set": {1, 2}, "bytes": b"blob", "nan": float("nan"), "surrogate": "\ud800"}[args]
