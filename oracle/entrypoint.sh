#!/bin/sh
# Oracle container entrypoint (AGPL-3.0-or-later, see oracle/LICENSE).
#
#   worker            one long-lived oracle worker on stdin/stdout (the default)
#   harness <args>    pool of workers for record/compare/diff-report (python -m driver.harness)
#   selftest [args]   driver self-tests, TAP on stdout (python -m driver.tests)
#
# A worker runs under libfaketime with a frozen clock read from
# $ORACLE_SCRATCH/clock on every clock call (FAKETIME_NO_CACHE). The driver
# writes epoch seconds there (FAKETIME_FMT=%s); it never uses the "@" form, so
# time only moves when a scenario moves it. Monotonic clocks stay real
# (DONT_FAKE_MONOTONIC), or timers in the sync client never fire.
# A timing worker (ORACLE_CLOCK_CACHE=1, tools/mg oracle time) keeps
# libfaketime's cache for its whole life instead: it reads the clock file,
# which the harness wrote before starting it, once (timed-operations.md T2.3).
set -eu

mode="${1:-worker}"
[ "$#" -gt 0 ] && shift

case "$mode" in
  worker)
    scratch="${ORACLE_SCRATCH:-/scratch/worker}"
    mkdir -p "$scratch"
    # 2000-01-01T00:00:00Z until the first scenario sets its own start.
    [ -s "$scratch/clock" ] || echo 946684800 >"$scratch/clock"
    case "$(dpkg --print-architecture)" in
      arm64) triplet=aarch64-linux-gnu ;;
      amd64) triplet=x86_64-linux-gnu ;;
      *) echo "oracle: unsupported architecture $(dpkg --print-architecture)" >&2; exit 70 ;;
    esac
    lib="/usr/lib/$triplet/faketime/libfaketime.so.1"
    [ -r "$lib" ] || { echo "oracle: $lib is missing" >&2; exit 70; }
    # Anki's test mode turns fuzz off and shifts the clock; the oracle must
    # behave like a user's Anki.
    unset ANKI_TEST_MODE
    export ORACLE_SCRATCH="$scratch" \
      LD_PRELOAD="$lib" \
      FAKETIME_TIMESTAMP_FILE="$scratch/clock" \
      FAKETIME_FMT=%s \
      DONT_FAKE_MONOTONIC=1
    if [ "${ORACLE_CLOCK_CACHE:-}" = 1 ]; then
      unset FAKETIME_NO_CACHE
      export FAKETIME_CACHE_DURATION=2147483647
    else
      unset FAKETIME_CACHE_DURATION
      export FAKETIME_NO_CACHE=1
    fi
    exec python3 -m driver "$@"
    ;;
  harness) exec python3 -m driver.harness "$@" ;;
  selftest) exec python3 -m driver.tests "$@" ;;
  *)
    echo "usage: entrypoint.sh [worker | harness <args> | selftest [args]]" >&2
    exit 64
    ;;
esac
