#!/bin/sh
# AnkiConnect recorder entrypoint (AGPL-3.0-or-later, see ../LICENSE).
#
#   anki    run the pinned Anki desktop app offscreen with AnkiConnect (default)
#   shell   sh, for probing
#
# The clock works as in the base oracle (../entrypoint.sh): libfaketime frozen
# at the epoch seconds in $ORACLE_SCRATCH/clock, read on every clock call, with
# real monotonic time so Qt timers and AnkiConnect's poll timer fire.
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
        FAKETIME_NO_CACHE=1 \
        DONT_FAKE_MONOTONIC=1
    fi
    export ORACLE_SCRATCH="$scratch" \
      QT_QPA_PLATFORM=offscreen \
      QTWEBENGINE_DISABLE_SANDBOX=1 \
      QTWEBENGINE_CHROMIUM_FLAGS="${QTWEBENGINE_CHROMIUM_FLAGS:---disable-gpu}" \
      XDG_RUNTIME_DIR="$scratch/xdg"
    mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
    exec python3 /opt/ankiconnect/launch.py "$@"
    ;;
  shell) exec sh "$@" ;;
  *)
    echo "usage: entrypoint.sh [anki | shell]" >&2
    exit 64
    ;;
esac
