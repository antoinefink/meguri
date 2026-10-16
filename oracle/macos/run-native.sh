#!/bin/bash
# macOS native oracle lane (KTD24, specs/conformance/float-determinism.md F6,
# oracle/macos/README.md). AGPL-3.0-or-later (oracle/LICENSE).
#
#   bootstrap          make or refresh build/oracle-macos-venv/: the pinned
#                      standalone CPython, the pinned libfaketime built from
#                      source, and a venv installed with hash checking from
#                      requirements-macos.lock
#   worker             one worker on stdin/stdout, like `entrypoint.sh worker`
#                      (TZ, ORACLE_SCRATCH and the other ORACLE_* variables)
#   hello [--tz Z]     one worker's hello response on stdout
#   harness <args>     python -m driver.harness natively (record-apple,
#                      compare-apple, time); its workers are `run-native.sh worker`
#
# A worker runs under libfaketime, injected with DYLD_INSERT_LIBRARIES and
# DYLD_FORCE_FLAT_NAMESPACE, with the container's clock-file variables. The
# interpreter is never /usr/bin/python3: macOS strips DYLD_* variables from
# protected binaries. The worker checks at start that the frozen clock reaches
# Python and Anki and answers time_control faketime or none.
# ORACLE_NATIVE_FAKETIME=0 starts a worker without libfaketime (time_control
# none), for testing that path.
#
# Runs under macOS /bin/bash 3.2.
set -eu

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
oracle="$(dirname "$here")"
repo="$(dirname "$oracle")"
lock="$here/requirements-macos.lock"
root="$repo/build/oracle-macos-venv"
venv="$root/venv"
python_home="$root/python"
faketime="$root/faketime/libfaketime.1.dylib"
stamp="$root/stamp"
scratch_root="$repo/build/oracle-macos-scratch"

# Pinned inputs: a release at least two weeks old, checked by sha256.
PY_VERSION=3.12.15
PY_URL="https://github.com/astral-sh/python-build-standalone/releases/download/20261001/cpython-3.12.15%2B20261001-aarch64-apple-darwin-install_only_stripped.tar.gz"
PY_SHA256=10cab8f6ed6202fdd81637aa6eda4af8d5b7eaa8fc42f9df3c6bea4923de0d93
FT_VERSION=0.9.13
FT_URL="https://github.com/wolfcw/libfaketime/archive/refs/tags/v$FT_VERSION.tar.gz"
FT_SHA256=8e56deeb805682b025107e095f1d94a6ea677472f05824cd475f5c5e6e1a5ddf

die() { # <exit code> <message>
  echo "run-native: $2" >&2
  exit "$1"
}

sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }

on_arm64_mac() {
  [ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] ||
    die 64 "the native oracle lane runs only on an arm64 Mac"
}

# The stamp names every input of the installed lane; a worker refuses a venv
# installed from anything else.
wanted_stamp() {
  printf 'python %s\nfaketime %s\nlock %s\n' "$PY_SHA256" "$FT_SHA256" "$(sha256 "$lock")"
}

current() {
  [ -f "$stamp" ] && [ -x "$venv/bin/python" ] && [ -f "$faketime" ] && [ "$(cat "$stamp")" = "$(wanted_stamp)" ]
}

# fetch <url> <sha256> <file>: download into its own directory, check the hash.
fetch() {
  local url="$1" want="$2" file="$3"
  if [ -f "$file" ] && [ "$(sha256 "$file")" = "$want" ]; then
    return 0
  fi
  rm -f "$file" "$file.part"
  mkdir -p "$(dirname "$file")"
  echo "run-native: downloading $url" >&2
  curl -fsSL --retry 3 -o "$file.part" "$url" || die 69 "cannot download $url"
  [ "$(sha256 "$file.part")" = "$want" ] || die 69 "$url does not have the pinned sha256 $want"
  mv "$file.part" "$file"
}

# take_lock: one bootstrap at a time per checkout (mkdir lock; a dead
# holder's lock is taken over).
take_lock() {
  local _ holder
  mkdir -p "$(dirname "$root")"
  for _ in $(seq 1200); do
    if mkdir "$root.lock" 2>/dev/null; then
      echo "$$" >"$root.lock/pid"
      trap 'rm -rf "$root.lock"' EXIT
      return 0
    fi
    holder="$(cat "$root.lock/pid" 2>/dev/null || true)"
    if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
      rm -rf "$root.lock"
      continue
    fi
    sleep 0.5
  done
  die 75 "another bootstrap holds $root.lock"
}

release_lock() {
  rm -rf "$root.lock"
  trap - EXIT
}

bootstrap() {
  on_arm64_mac
  [ -f "$lock" ] || die 66 "$lock is missing"
  take_lock
  if current; then
    echo "run-native: $root is up to date" >&2
    release_lock
    return 0
  fi
  rm -f "$stamp"
  local downloads="$root/downloads" old
  old="$(grep '^python ' "$stamp.prev" 2>/dev/null || true)"
  if [ ! -x "$python_home/bin/python3" ] || [ "$old" != "python $PY_SHA256" ]; then
    fetch "$PY_URL" "$PY_SHA256" "$downloads/python/python.tar.gz"
    rm -rf "$root/python.new" && mkdir -p "$root/python.new"
    tar -xzf "$downloads/python/python.tar.gz" -C "$root/python.new"
    [ -x "$root/python.new/python/bin/python3" ] || die 70 "the Python archive has no python/bin/python3"
    rm -rf "$python_home" "$venv"
    mv "$root/python.new/python" "$python_home"
    rm -rf "$root/python.new"
  fi
  [ "$("$python_home/bin/python3" -c 'import platform; print(platform.python_version())')" = "$PY_VERSION" ] ||
    die 70 "$python_home is not CPython $PY_VERSION"
  old="$(grep '^faketime ' "$stamp.prev" 2>/dev/null || true)"
  if [ ! -f "$faketime" ] || [ "$old" != "faketime $FT_SHA256" ]; then
    fetch "$FT_URL" "$FT_SHA256" "$downloads/faketime/libfaketime.tar.gz"
    rm -rf "$root/faketime-src" && mkdir -p "$root/faketime-src"
    tar -xzf "$downloads/faketime/libfaketime.tar.gz" -C "$root/faketime-src"
    (cd "$root/faketime-src/libfaketime-$FT_VERSION/src" &&
      make -f Makefile.OSX PREFIX="$root/faketime" libfaketime.1.dylib >&2) ||
      die 70 "building libfaketime $FT_VERSION failed"
    mkdir -p "$root/faketime"
    cp "$root/faketime-src/libfaketime-$FT_VERSION/src/libfaketime.1.dylib" "$faketime"
    rm -rf "$root/faketime-src"
  fi
  rm -rf "$venv"
  "$python_home/bin/python3" -m venv "$venv"
  PIP_CONFIG_FILE=/dev/null PYTHONNOUSERSITE=1 "$venv/bin/python" -m pip install --quiet --disable-pip-version-check \
    --no-input --require-hashes --no-deps --only-binary :all: -r "$lock" >&2 ||
    die 70 "pip could not install $lock"
  "$venv/bin/python" -s -c 'import anki.collection' || die 70 "the venv cannot import anki"
  wanted_stamp >"$stamp"
  cp "$stamp" "$stamp.prev"
  release_lock
  echo "run-native: $root is ready (CPython $PY_VERSION, libfaketime $FT_VERSION)" >&2
}

require_current() {
  on_arm64_mac
  current || die 78 "$root is missing or was installed from other pins; run oracle/macos/run-native.sh bootstrap"
}

python_env() {
  export ORACLE_HOME="$oracle" PYTHONPATH="$oracle" PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 \
    PYTHONNOUSERSITE=1 PYTHONSAFEPATH=1
}

worker() {
  require_current
  local scratch="${ORACLE_SCRATCH:-$scratch_root/worker}"
  mkdir -p "$scratch"
  scratch="$(cd "$scratch" && pwd -P)"
  # 2000-01-01T00:00:00Z until the first scenario sets its own start.
  [ -s "$scratch/clock" ] || echo 946684800 >"$scratch/clock"
  # Anki's test mode turns fuzz off and shifts the clock.
  unset ANKI_TEST_MODE
  python_env
  export ORACLE_SCRATCH="$scratch" \
    FAKETIME_TIMESTAMP_FILE="$scratch/clock" \
    FAKETIME_FMT=%s \
    DONT_FAKE_MONOTONIC=1
  # A timing worker reads its clock file once (entrypoint.sh, T2.3).
  if [ "${ORACLE_CLOCK_CACHE:-}" = 1 ]; then
    unset FAKETIME_NO_CACHE
    export FAKETIME_CACHE_DURATION=2147483647
  else
    unset FAKETIME_CACHE_DURATION
    export FAKETIME_NO_CACHE=1
  fi
  if [ "${ORACLE_NATIVE_FAKETIME:-1}" = 0 ]; then
    unset DYLD_INSERT_LIBRARIES DYLD_FORCE_FLAT_NAMESPACE
  else
    export DYLD_INSERT_LIBRARIES="$faketime" DYLD_FORCE_FLAT_NAMESPACE=1
  fi
  cd "$scratch"
  exec "$venv/bin/python" -m driver "$@"
}

hello() {
  local tz=UTC dir rc
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --tz) [ "$#" -ge 2 ] || die 64 "--tz needs a zone"; tz="$2"; shift 2 ;;
      *) die 64 "hello: unexpected argument '$1'" ;;
    esac
  done
  bootstrap
  mkdir -p "$scratch_root"
  dir="$(mktemp -d "$scratch_root/hello-XXXXXX")"
  rc=0
  printf '{"cmd":"hello"}\n' | TZ="$tz" ORACLE_SCRATCH="$dir" "$here/run-native.sh" worker || rc=$?
  rm -rf "$dir"
  return "$rc"
}

harness() {
  require_current
  python_env
  export ORACLE_SCRATCH_ROOT="${ORACLE_SCRATCH_ROOT:-$scratch_root}"
  mkdir -p "$ORACLE_SCRATCH_ROOT"
  unset DYLD_INSERT_LIBRARIES DYLD_FORCE_FLAT_NAMESPACE
  exec "$venv/bin/python" -m driver.harness "$@"
}

mode="${1:-}"
[ "$#" -gt 0 ] && shift
case "$mode" in
  bootstrap) [ "$#" -eq 0 ] || die 64 "bootstrap takes no arguments"; bootstrap ;;
  worker) worker "$@" ;;
  hello) hello "$@" ;;
  harness) harness "$@" ;;
  *) die 64 "usage: run-native.sh bootstrap | worker | hello [--tz Z] | harness <args>" ;;
esac
