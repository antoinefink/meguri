# SPDX-License-Identifier: MIT
"""Wait for the actual service listening event; never retry HTTP requests."""
import json
import os
import pathlib
import selectors
import stat
import subprocess
import sys
import time

if sys.argv[1] == '--password-file':
    path = pathlib.Path(sys.argv[2])
    metadata = path.lstat()
    if not (stat.S_ISREG(metadata.st_mode) and metadata.st_uid == os.getuid()
            and stat.S_IMODE(metadata.st_mode) == 0o600 and metadata.st_nlink == 1):
        raise SystemExit('Recovery needs an owned regular mode-0600 password file')
    raise SystemExit(0)

process = subprocess.Popen(['docker', 'logs', '--follow', sys.argv[1]], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
selector = selectors.DefaultSelector()
selector.register(process.stdout, selectors.EVENT_READ)
buffer = b''
deadline = time.monotonic() + 30
try:
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0 or not selector.select(remaining):
            raise ValueError('Service did not publish its listening event')
        chunk = os.read(process.stdout.fileno(), 4096)
        if not chunk:
            raise ValueError('Service exited before listening')
        buffer = (buffer + chunk)[-32768:]
        if b'listening 0.0.0.0:8080' in buffer:
            inspection = json.loads(subprocess.check_output(['docker', 'inspect', sys.argv[1]]))
            if len(inspection) != 1 or not inspection[0]['State']['Running']:
                raise ValueError('Service is no longer running')
            break
finally:
    selector.close()
    # Stop only our log follower, never signal a service process here.
    process.terminate()
    process.wait(timeout=5)
