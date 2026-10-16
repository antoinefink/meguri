#!/bin/sh
# In-container wrapper for make.py (AGPL-3.0-or-later, oracle/LICENSE): the
# environment of an oracle worker (entrypoint.sh worker), so the recipe runs
# under libfaketime with a frozen clock file the driver moves, in UTC.
set -eu
scratch="${ORACLE_SCRATCH:-/scratch/maker}"
mkdir -p "$scratch"
[ -s "$scratch/clock" ] || echo 946684800 >"$scratch/clock"
case "$(dpkg --print-architecture)" in
  arm64) triplet=aarch64-linux-gnu ;;
  amd64) triplet=x86_64-linux-gnu ;;
  *) echo "run.sh: unsupported architecture" >&2; exit 70 ;;
esac
unset ANKI_TEST_MODE
export ORACLE_SCRATCH="$scratch" \
  LD_PRELOAD="/usr/lib/$triplet/faketime/libfaketime.so.1" \
  FAKETIME_TIMESTAMP_FILE="$scratch/clock" \
  FAKETIME_NO_CACHE=1 \
  FAKETIME_FMT=%s \
  DONT_FAKE_MONOTONIC=1 \
  TZ=UTC
exec python3 "$(dirname "$0")/make.py" "$@"
