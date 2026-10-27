#!/bin/sh
# In-container wrapper for oldclient.py (2.1.26 legacy image): libfaketime with
# the clock frozen at 2026-09-01T08:00:00Z, moved only by oldclient.py's
# set_clock. AGPL-3.0-or-later (oracle/LICENSE).
set -eu
clock="$(mktemp)"
echo 1788249600 >"$clock"
case "$(dpkg --print-architecture)" in
  arm64) triplet=aarch64-linux-gnu ;;
  amd64) triplet=x86_64-linux-gnu ;;
  *) echo "oldclient-run.sh: unsupported architecture" >&2; exit 70 ;;
esac
export LD_PRELOAD="/usr/lib/$triplet/faketime/libfaketime.so.1" \
  FAKETIME_TIMESTAMP_FILE="$clock" FAKETIME_NO_CACHE=1 FAKETIME_FMT=%s DONT_FAKE_MONOTONIC=1 TZ=UTC
unset ANKI_TEST_MODE
exec python3 "$(dirname "$0")/oldclient.py" "$@"
