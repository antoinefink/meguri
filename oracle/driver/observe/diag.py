# SPDX-License-Identifier: AGPL-3.0-or-later
"""net_probe: {host, port, timeout_ms?} -> {connected, error}.

Tries one TCP connection from inside the worker. Used to show that oracle
containers have no route out (R1876); error is the errno name or "timeout".
"""

from __future__ import annotations

import errno
import socket

from ..errors import invalid
from ..registry import observation


@observation("net_probe")
def net_probe(session, args):
    if not isinstance(args, dict) or not isinstance(args.get("host"), str) or not isinstance(args.get("port"), int):
        raise invalid("net_probe takes {host, port, timeout_ms?}")
    timeout = args.get("timeout_ms", 2000) / 1000
    try:
        with socket.create_connection((args["host"], args["port"]), timeout=timeout):
            return {"connected": True, "error": None}
    except socket.timeout:
        return {"connected": False, "error": "timeout"}
    except OSError as exc:
        return {"connected": False, "error": errno.errorcode.get(exc.errno, "oserror") if exc.errno else type(exc).__name__}
