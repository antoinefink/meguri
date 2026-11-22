#!/usr/bin/env bats
# mg_host_build (tools/scripts/host-build.sh): host builds run under
# taskpolicy's utility clamp with a bounded job count. Fakes on PATH record
# what would run.

setup() {
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  for p in taskpolicy xcodebuild swift getconf; do
    cat >"$BIN/$p" <<'EOF'
#!/bin/bash
name="${0##*/}"
if [ "$name" = getconf ]; then echo 18; exit 0; fi
echo "$name $* CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-} MEGURI_BUILD_JOBS=${MEGURI_BUILD_JOBS:-}"
[ "$name" != taskpolicy ] || { shift 2; exec "$@"; }
EOF
    chmod +x "$BIN/$p"
  done
  # shellcheck source=/dev/null
  . "$BATS_TEST_DIRNAME/../scripts/host-build.sh"
  PATH="$BIN:$PATH"
  unset MEGURI_BUILD_JOBS CI
}

@test "xcodebuild runs clamped with -jobs at half the cores" {
  run mg_host_build xcodebuild build -scheme X
  [ "$status" -eq 0 ]
  [ "${lines[0]}" = "taskpolicy -c utility xcodebuild build -jobs 9 -scheme X CARGO_BUILD_JOBS=9 MEGURI_BUILD_JOBS=9" ]
  [ "${lines[1]}" = "xcodebuild build -jobs 9 -scheme X CARGO_BUILD_JOBS=9 MEGURI_BUILD_JOBS=9" ]
}

@test "MEGURI_BUILD_JOBS overrides; swift build gets -j, swift test does not" {
  MEGURI_BUILD_JOBS=3 run mg_host_build swift build --package-path p
  [ "${lines[1]}" = "swift build --package-path p -j 3 CARGO_BUILD_JOBS=3 MEGURI_BUILD_JOBS=3" ]
  MEGURI_BUILD_JOBS=bad run mg_host_build swift test
  [ "${lines[1]}" = "swift test CARGO_BUILD_JOBS=9 MEGURI_BUILD_JOBS=9" ]
}

@test "on CI or without taskpolicy the build runs unclamped" {
  CI=true run mg_host_build xcodebuild build
  [ "$output" = "xcodebuild build -jobs 18 CARGO_BUILD_JOBS=18 MEGURI_BUILD_JOBS=18" ]
  rm "$BIN/taskpolicy"
  PATH="$BIN" run mg_host_build xcodebuild build
  [ "$output" = "xcodebuild build -jobs 9 CARGO_BUILD_JOBS=9 MEGURI_BUILD_JOBS=9" ]
}
