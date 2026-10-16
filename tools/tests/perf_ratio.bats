#!/usr/bin/env bats
# Self-tests for tools/mg perf-ratio (U18: R1806, R1865;
# specs/conformance/timed-operations.md T11): the nightly run on generated
# collections in both lanes, the private run on a vault copy, the row each
# operation gets in docs/process/perf-ratio.csv (ok, fail, pending,
# mismatch, error) and the failure rule (Meguri not faster fails). The timed
# runs are faked: the nested `tools/mg oracle time` runs its real result
# handling on a canned timing.json, and meguri-cli's build and runs, the
# fixtures and the vault copy are stand-ins. The private scan is real.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
HEADER="date,commit,platform,collection,cards,operation,anki_ms,meguri_ms,ratio,status"
COMMIT="0123456789abcdef0123456789abcdef01234567"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core" "$t/docs/process"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/core/rust-toolchain.toml" "$t/core/"
  cp "$REPO_ROOT/docs/process/perf-ratio.csv" "$t/docs/process/"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  SB="$(cd "$SB" && pwd -P)"
  export SB STUB_LOG="$BATS_TEST_TMPDIR/stub.log" FX="$BATS_TEST_TMPDIR/fx"
  export HOME="$BATS_TEST_TMPDIR/home" TMPDIR="$BATS_TEST_TMPDIR/tmp"
  export MEGURI_HOST_LEASE_DIR="$BATS_TEST_TMPDIR/host.lock" MEGURI_HOST_LEASE_POLL_SECONDS=1
  for v in CI CONTINUOUS_INTEGRATION BUILDKITE BITBUCKET_BUILD_NUMBER CIRCLECI CI_XCODE_PROJECT CODEBUILD_BUILD_ID DRONE GITHUB_ACTIONS GITLAB_CI JENKINS_URL TEAMCITY_VERSION TF_BUILD TRAVIS; do
    unset "$v"
  done
  mkdir -p "$HOME" "$TMPDIR" "$FX"
  : >"$STUB_LOG"
  BIN="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$BIN"
  printf '#!/bin/bash\nexec sleep 600\n' >"$BIN/caffeinate" && chmod +x "$BIN/caffeinate"
  export PATH="$BIN:$PATH"
  add_fakes
}

teardown() {
  pkill -f "$BATS_TEST_TMPDIR/bin/caffeinate" 2>/dev/null || true
  [ -z "${result:-}" ] || [ ! -f "$result" ] || jq -c '{status, message, failures}' "$result"
  [ -z "${stderr:-}" ] || printf '%s\n' "$stderr" | tail -n 30
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }
called() { grep -Eq -- "$1" "$STUB_LOG" || { echo "no call matching: $1"; cat "$STUB_LOG"; return 1; }; }
rows() { tail -n +2 "$SB/docs/process/perf-ratio.csv"; }

# The fixtures are directories named fx-<size>; the vault copy is .../copy.
# The oracle's `time` job and meguri-cli's runs answer with $FX/anki-<lane>-
# <label>.json and $FX/meguri-<lane>-<label>.json (lane apple or linux,
# label the size or `private`), or with an error report when there is none.
add_fakes() {
  cat >"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
fx_label() { case "$1" in */copy) echo private ;; *) echo "${1##*-}" ;; esac; }
mg__perf_ratio_fixtures() {
  local size
  echo "fake fixtures $*" >>"$STUB_LOG"
  [ -z "${FX_FIXTURES_FAIL:-}" ] || return 1
  for size in "$@"; do
    mkdir -p "$MG_ROOT/build/fixtures/fx-$size"
    : >"$MG_ROOT/build/fixtures/fx-$size/collection.anki2"
    printf '%s\t%s\n' "$size" "$MG_ROOT/build/fixtures/fx-$size"
  done
}
mg__perf_ratio_build() { echo "fake build $1 commit=$2" >>"$STUB_LOG"; return "${FX_BUILD_RC:-0}"; }
mg__perf_ratio_meguri() {
  local lane="$1" dir="$2" out="$3" fx
  shift 3
  echo "fake meguri $lane $dir${*:+ $*}" >>"$STUB_LOG"
  fx="$FX/meguri-$lane-$(fx_label "$dir").json"
  mkdir -p "$out"
  if [ -f "$fx" ]; then
    cp "$fx" "$out/timing.json"
  else
    printf '{"detail": "made up", "kind": "failed"}\n' >"$out/timing-error.json"
    return 1
  fi
}
mg_oracle_time() {
  local dir="$1" lane=linux fx
  echo "fake oracle time $*" >>"$STUB_LOG"
  case " $* " in *" --platform apple "*) lane=apple ;; esac
  fx="$FX/anki-$lane-$(fx_label "$dir").json"
  if [ -f "$fx" ]; then
    cp "$fx" "$MG_RUN_DIR/timing.json"
    mg_oracle__time_result 0 "$dir"
  else
    printf '{"detail": "made up", "kind": "oracle_error"}\n' >"$MG_RUN_DIR/timing-error.json"
    mg_oracle__time_result 2 "$dir"
  fi
}
mg__private_hooks() { echo "fake hooks" >>"$STUB_LOG"; }
mg__private_docker() { echo "fake private docker" >>"$STUB_LOG"; }
mg__private_copy() {
  echo "fake vault copy $1" >>"$STUB_LOG"
  mkdir -p "$MG_ROOT/$1/copy"
  : >"$MG_ROOT/$1/copy/collection.anki2"
}
EOF
}

# report <anki|meguri> <platform> <cards> <name=status[:median_ms[:check cards]]>...
# A T7 report on a made-up collection of <cards> cards, one sample per
# operation, every check {cards: <check cards, default the collection's>}.
report() {
  local side="$1" platform="$2" cards="$3" ops="[]" spec name rest status median check
  shift 3
  for spec in "$@"; do
    name="${spec%%=*}"
    rest="${spec#*=}"
    IFS=: read -r status median check <<<"$rest"
    ops="$(jq -c --arg n "$name" --arg s "$status" --arg m "${median:-0}" --arg c "${check:-$cards}" '. + [
      {name: $n, status: $s}
      + (if $s == "ok" then {samples_ms: [($m | tonumber)], statistic: {median_ms: ($m | tonumber), min_ms: ($m | tonumber)},
           check: {cards: ($c | tonumber)}}
         elif $s == "error" then {error: {kind: "failed", stage: "timed", exception: "ValueError"}}
         else {} end)]' <<<"$ops")"
  done
  jq -n --arg side "$side" --arg p "$platform" --argjson cards "$cards" --argjson ops "$ops" --arg commit "$COMMIT" '
    {report_version: 1, side: $side, platform: $p, protocol: {warmup: 1, repeat: 5}, now: 1894701600, tz: "Etc/GMT-1",
     collection: {notes: 10, cards: $cards, revlog: 20, notetypes: 2, decks: 3, presets: 1, tags: 4, graves: 0},
     operations: $ops}
    + (if $side == "anki" then {oracle: {anki_version: "26.9.3", driver_sha256: ("a" * 64), lock_sha256: ("b" * 64)}}
       else {meguri: {commit: $commit, profile: "release"}} end)'
}

# pair <lane> <label> <cards> <anki specs> -- <meguri specs>
pair() {
  local lane="$1" label="$2" cards="$3" platform=linux-arm64 anki=() side=anki
  shift 3
  [ "$lane" = linux ] || platform=macos-arm64
  for spec in "$@"; do
    if [ "$spec" = -- ]; then
      report anki "$platform" "$cards" ${anki[0]+"${anki[@]}"} >"$FX/anki-$lane-$label.json"
      side=meguri
      anki=()
      continue
    fi
    anki+=("$spec")
  done
  report meguri "$platform" "$cards" ${anki[0]+"${anki[@]}"} >"$FX/meguri-$lane-$label.json"
}

@test "perf-ratio: the nightly run appends a row per operation and lane, with both times and the ratio" {
  pair apple 10k 9953 collection.open=ok:2.0 decks.counts=ok:4.25 -- collection.open=ok:1.0 decks.counts=ok:4.249
  pair linux 10k 9953 collection.open=ok:3.0 decks.counts=ok:6.0 -- collection.open=ok:0.5 decks.counts=ok:5.0
  mg perf-ratio --sizes 10k
  [ "$(field .status)" = pass ]
  local today
  today="$(date -u +%Y-%m-%d)"
  [ "$(head -n 1 "$SB/docs/process/perf-ratio.csv")" = "$HEADER" ]
  [ "$(rows)" = "$today,$COMMIT,macos-arm64,10k,9953,collection.open,2.000,1.000,0.500,ok
$today,$COMMIT,macos-arm64,10k,9953,decks.counts,4.250,4.249,1.000,ok
$today,$COMMIT,linux-arm64,10k,9953,collection.open,3.000,0.500,0.167,ok
$today,$COMMIT,linux-arm64,10k,9953,decks.counts,6.000,5.000,0.833,ok" ]
  [ "$(field '.details.rows | length')" -eq 4 ]
  [ "$(field '.summary.total')" -eq 4 ]
  # The native lane first (the primary series), then the Docker lane; the
  # oracle runs natively only for the native lane.
  called "^fake fixtures 10k$"
  called "^fake build apple commit="
  called "^fake build linux commit="
  called "^fake oracle time $SB/build/fixtures/fx-10k --platform apple$"
  called "^fake oracle time $SB/build/fixtures/fx-10k$"
  called "^fake meguri apple $SB/build/fixtures/fx-10k$"
  called "^fake meguri linux $SB/build/fixtures/fx-10k$"
  [ "$(grep -n 'fake oracle time .* --platform apple' "$STUB_LOG" | cut -d: -f1)" -lt "$(grep -n 'fake meguri linux' "$STUB_LOG" | cut -d: -f1)" ]
  [ "$(field '.details.host_lease.job')" = perf-ratio ]
}

@test "perf-ratio: an unwritable history path reports an error instead of success" {
  pair apple 10k 9953 collection.open=ok:2.0 -- collection.open=ok:1.0
  # A directory refuses writes even when the dev container runs as root.
  rm "$SB/docs/process/perf-ratio.csv"
  mkdir "$SB/docs/process/perf-ratio.csv"
  mg perf-ratio --sizes 10k --platform apple
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = history-write ]
  [ "$(field '.summary.total')" -eq 0 ]
}

@test "perf-ratio: a failed row append reports an error instead of counting rows" {
  pair apple 10k 9953 collection.open=ok:2.0 -- collection.open=ok:1.0
  cat >>"$SB/tools/mg.d/zz-fakes.sh" <<'EOF'
cat() {
  if [ "$1" = "$MG_RUN_DIR/rows.csv" ]; then return 1; fi
  command cat "$@"
}
EOF
  mg perf-ratio --sizes 10k --platform apple
  [ "$status" -eq 2 ]
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = history-write ]
  [ "$(field '.summary.total')" -eq 0 ]
  [ -z "$(rows)" ]
}

@test "perf-ratio: an operation Meguri lacks is pending, with Anki's time only, and fails nothing" {
  pair apple 10k 9953 collection.open=ok:2.0 search.deck=ok:3.5 -- collection.open=unsupported search.deck=unsupported
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = pass ]
  [ "$(rows | cut -d, -f3-)" = "macos-arm64,10k,9953,collection.open,2.000,,,pending
macos-arm64,10k,9953,search.deck,3.500,,,pending" ]
  [ "$(field '.failures | length')" -eq 0 ]
  ! called "fake build linux"
  ! called "fake meguri linux"
}

@test "perf-ratio: Meguri not faster than Anki fails, naming the operation" {
  pair apple 10k 9953 collection.open=ok:2.0 decks.counts=ok:4.0 queue.build=ok:1.0 -- \
    collection.open=ok:2.5 decks.counts=ok:4.0 queue.build=ok:0.9
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = fail ]
  [ "$(rows | cut -d, -f6-)" = "collection.open,2.000,2.500,1.250,fail
decks.counts,4.000,4.000,1.000,fail
queue.build,1.000,0.900,0.900,ok" ]
  [ "$(field '[.failures[].id] | join(" ")')" = "macos-arm64/10k/collection.open macos-arm64/10k/decks.counts" ]
  field '.failures[0].message' | grep -q 'collection.open'
  field '.failures[0].message' | grep -q '1.250'
}

@test "perf-ratio: checks that differ give no ratio and fail as a mismatch" {
  pair apple 10k 9953 search.tag=ok:2.0:120 -- search.tag=ok:1.0:119
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = fail ]
  [ "$(rows | cut -d, -f6-)" = "search.tag,2.000,1.000,,mismatch" ]
  [ "$(field '.failures[0].id')" = macos-arm64/10k/search.tag ]
  field '.failures[0].message' | grep -q 'mismatch'
}

@test "perf-ratio: an error on either side, or an unsupported oracle operation, gives no ratio and fails" {
  pair apple 10k 9953 collection.open=error decks.counts=ok:2.0 queue.build=unsupported -- \
    collection.open=ok:1.0 decks.counts=error queue.build=ok:1.0
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = fail ]
  [ "$(rows | cut -d, -f6-)" = "collection.open,,1.000,,error
decks.counts,2.000,,,error
queue.build,,1.000,,error" ]
  [ "$(field '.failures | length')" -eq 3 ]
}

@test "perf-ratio: reports that are not comparable, or a side without a report, append nothing and end in error" {
  pair apple 10k 9953 collection.open=ok:2.0 -- collection.open=ok:1.0
  jq '.collection.cards = 9954' "$FX/meguri-apple-10k.json" >"$FX/m.json" && mv "$FX/m.json" "$FX/meguri-apple-10k.json"
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = incomparable/macos-arm64/10k ]
  field '.failures[0].message' | grep -q collection
  [ -z "$(rows)" ]

  pair apple 10k 9953 collection.open=ok:2.0 -- collection.open=ok:1.0
  jq '.meguri.profile = "dev"' "$FX/meguri-apple-10k.json" >"$FX/m.json" && mv "$FX/m.json" "$FX/meguri-apple-10k.json"
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = error ]
  field '.failures[0].message' | grep -q release
  [ -z "$(rows)" ]

  rm "$FX/meguri-apple-10k.json"
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = meguri/macos-arm64/10k ]
  field '.failures[0].message' | grep -q failed
  [ -z "$(rows)" ]

  pair apple 10k 9953 collection.open=ok:2.0 -- collection.open=ok:1.0
  rm "$FX/anki-apple-10k.json"
  : >"$STUB_LOG"
  mg perf-ratio --sizes 10k --platform apple
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = anki/macos-arm64/10k ]
  field '.failures[0].message' | grep -q oracle_error
  [ -z "$(rows)" ]
  ! called "fake meguri"
}

@test "perf-ratio: --op, --repeat, --now and --tz reach both sides" {
  pair apple 10k 9953 collection.open=ok:2.0 -- collection.open=ok:1.0
  mg perf-ratio --sizes 10k --platform apple --op collection.open --repeat 3 --now 2030-01-15T10:00:00Z --tz UTC
  [ "$(field .status)" = pass ]
  called "^fake oracle time $SB/build/fixtures/fx-10k --platform apple --op collection.open --repeat 3 --now 2030-01-15T10:00:00Z --tz UTC$"
  called "^fake meguri apple $SB/build/fixtures/fx-10k --op collection.open --repeat 3 --now 2030-01-15T10:00:00Z --tz UTC$"
}

@test "perf-ratio: --private times a fresh vault copy natively at noon and records aggregate rows only" {
  mkdir -p "$SB/local-data/anki" "$SB/build/private/tokens"
  : >"$SB/local-data/anki/collection.anki2"
  # A made-up token set; the rows hold none of its words.
  printf 'quokkasprung\nzebrafink hörverstehen\n' >"$SB/build/private/tokens/tokens.txt"
  pair apple private 52311 collection.open=ok:30.0 search.text=ok:12.0 -- collection.open=ok:20.0 search.text=unsupported
  mg perf-ratio --private --tz UTC
  [ "$(field .status)" = pass ]
  local run today
  run="$(field .run_id)"
  today="$(date -u +%Y-%m-%d)"
  [ "$(rows)" = "$today,$COMMIT,macos-arm64,private,52311,collection.open,30.000,20.000,0.667,ok
$today,$COMMIT,macos-arm64,private,52311,search.text,12.000,,,pending" ]
  called "^fake vault copy build/private/$run$"
  called "^fake oracle time $SB/build/private/$run/copy --platform apple --now ${today}T12:00:00\\+00:00 --tz UTC$"
  called "^fake meguri apple $SB/build/private/$run/copy --now ${today}T12:00:00\\+00:00 --tz UTC$"
  ! called "fake build linux"
  ! called "fake fixtures"
  [ ! -e "$SB/build/private/$run/copy" ]
  mg private scan-outputs docs/process/perf-ratio.csv
  [ "$(field .status)" = pass ]
}

@test "perf-ratio: --private with Meguri slower fails naming the operation (R1806)" {
  mkdir -p "$SB/local-data/anki" "$SB/build/private/tokens"
  : >"$SB/local-data/anki/collection.anki2"
  printf 'quokkasprung\n' >"$SB/build/private/tokens/tokens.txt"
  pair apple private 52311 review.answer=ok:0.4 -- review.answer=ok:0.5
  mg perf-ratio --private --tz UTC
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = macos-arm64/private/review.answer ]
  [ "$(rows | cut -d, -f4-)" = "private,52311,review.answer,0.400,0.500,1.250,fail" ]
}

@test "perf-ratio: --private rows that hold a private token are not recorded" {
  mkdir -p "$SB/local-data/anki" "$SB/build/private/tokens"
  : >"$SB/local-data/anki/collection.anki2"
  # A token set that matches the rows (`collection` of collection.open).
  printf 'collection\n' >"$SB/build/private/tokens/tokens.txt"
  pair apple private 52311 collection.open=ok:30.0 -- collection.open=ok:20.0
  mg perf-ratio --private --tz UTC
  [ "$(field .status)" = fail ]
  [ "$(field '.failures[0].id')" = rows-private-token ]
  [ -z "$(rows)" ]
}

@test "perf-ratio: --private is refused on CI and in implementer worktrees, before any copy" {
  mkdir -p "$SB/local-data/anki"
  : >"$SB/local-data/anki/collection.anki2"
  CI=true mg perf-ratio --private --tz UTC
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = ci-marker ]
  printf '{"role": "implementer"}\n' >"$SB/.git/meguri-session.json"
  mg perf-ratio --private --tz UTC
  [ "$(field '.failures[0].id')" = implementer-worktree ]
  ! called "fake vault copy"
  ! called "fake oracle time"
}

@test "perf-ratio: usage errors and an unknown history header stop before anything runs" {
  local args
  for args in "--private --sizes 10k" "--private --platform linux" "--platform windows" "--sizes" "--bogus" "--repeat"; do
    # shellcheck disable=SC2086 # one argument per word
    mg perf-ratio $args
    [ "$(field .status)" = error ]
    [ "$(field '.failures[0].id')" = usage ]
  done
  printf 'date,operation,ratio\n' >"$SB/docs/process/perf-ratio.csv"
  mg perf-ratio --sizes 10k
  [ "$(field .status)" = error ]
  [ "$(field '.failures[0].id')" = history-header ]
  [ ! -s "$STUB_LOG" ]
}
