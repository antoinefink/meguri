#!/bin/sh
# AnkiConnect recorder entrypoint (AGPL-3.0-or-later, see ../LICENSE).
#
#   anki    run the pinned Anki desktop app offscreen with AnkiConnect (default)
#   record  record one transcript scenario: python -m recorder (see
#           recorder/__main__.py); the scenario on stdin, the golden on stdout.
#           The recorder itself runs on real time and starts `anki` itself.
#   test    the recorder's unit tests (no Anki)
#   shell   sh, for probing
#
# The clock works as in the base oracle (../entrypoint.sh): libfaketime frozen
# at the epoch seconds in $ORACLE_SCRATCH/clock, read on every clock call (or
# every ORACLE_FAKETIME_CACHE_SECS, below), with real monotonic time so Qt
# timers and AnkiConnect's poll timer fire.
set -eu

mode="${1:-anki}"
[ "$#" -gt 0 ] && shift

case "$mode" in
  anki)
    scratch="${ORACLE_SCRATCH:-/scratch/worker}"
    mkdir -p "$scratch"
    [ -s "$scratch/clock" ] || echo "${ORACLE_CLOCK_START:-946684800}" >"$scratch/clock"
    case "$(dpkg --print-architecture)" in
      arm64) triplet=aarch64-linux-gnu ;;
      amd64) triplet=x86_64-linux-gnu ;;
      *) echo "recorder: unsupported architecture" >&2; exit 70 ;;
    esac
    lib="/usr/lib/$triplet/faketime/libfaketime.so.1"
    [ -r "$lib" ] || { echo "recorder: $lib is missing" >&2; exit 70; }
    unset ANKI_TEST_MODE
    if [ "${ORACLE_NO_FAKETIME:-}" != 1 ]; then
      export LD_PRELOAD="$lib" \
        FAKETIME_TIMESTAMP_FILE="$scratch/clock" \
        FAKETIME_FMT=%s \
        DONT_FAKE_MONOTONIC=1
      # libfaketime 0.9.10 re-reads the clock file on every clock call with
      # FAKETIME_NO_CACHE, and Anki's many threads race on that read: one
      # recording in about a dozen got a real-time card id. The recorder sets
      # ORACLE_FAKETIME_CACHE_SECS=1 (a re-read once the whole-second
      # difference exceeds it), waits after each advance until Anki's clock
      # reads the new instant, and checks every new id and mod
      # (clock_escaped).
      if [ -n "${ORACLE_FAKETIME_CACHE_SECS:-}" ]; then
        export FAKETIME_CACHE_DURATION="$ORACLE_FAKETIME_CACHE_SECS"
      else
        export FAKETIME_NO_CACHE=1
      fi
    fi
    export ORACLE_SCRATCH="$scratch" \
      QT_QPA_PLATFORM=offscreen \
      QTWEBENGINE_DISABLE_SANDBOX=1 \
      QTWEBENGINE_CHROMIUM_FLAGS="${QTWEBENGINE_CHROMIUM_FLAGS:---disable-gpu}" \
      XDG_RUNTIME_DIR="$scratch/xdg"
    mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
    exec python3 /opt/ankiconnect/launch.py "$@"
    ;;
  record)
    PYTHONPATH="/opt/ankiconnect:${PYTHONPATH:-/opt/oracle}" exec python3 -m recorder "$@"
    ;;
  test)
    cd /opt/ankiconnect
    PYTHONPATH="/opt/ankiconnect:${PYTHONPATH:-/opt/oracle}" exec python3 -m unittest discover -s recorder -p 'test_*.py' "$@"
    ;;
  shell) exec sh "$@" ;;
  *)
    echo "usage: entrypoint.sh [anki | record | test | shell]" >&2
    exit 64
    ;;
esac
