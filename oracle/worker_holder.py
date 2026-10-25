# SPDX-License-Identifier: AGPL-3.0-or-later
"""Keep one mixed worker's namespace reserved until its caller removes it.

Run with the real clock as ``python3 -u /opt/oracle/worker_holder.py``.
The caller starts the sole protocol driver separately through
``/opt/oracle/entrypoint.sh worker`` and owns container teardown. This holder
has no protocol, listeners or replacement-driver machinery.
"""

import signal


def finish_holder(_signal_number, _signal_context):
    raise SystemExit(0)


def main():
    signal.signal(signal.SIGTERM, finish_holder)
    signal.signal(signal.SIGINT, finish_holder)
    while True:
        signal.pause()


if __name__ == "__main__":
    main()
