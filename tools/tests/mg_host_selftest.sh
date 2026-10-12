#!/bin/bash
# Host self-tests for tools/mg, run by `tools/mg selftest-host`; prints TAP.
# They cover what the container suite cannot: macOS /bin/bash 3.2 and BSD
# tools, the real doctor on this Mac, a docker job started from the host, and
# a dev image rebuild that reuses every layer.
#
# usage: mg_host_selftest.sh <sandbox dir>
# The sandbox is a throwaway copy of tools/ and docker/ in its own git repo
# and its own compose project; its image tags, volume and network are removed
# at exit.

set -u

REPO="$(cd "$(dirname "$0")/../.." && pwd -P)"
mkdir -p "$1"
SB="$(cd "$1" && pwd -P)"
MG="$SB/tools/mg"
RESULTS=""
# Same naming rule as mg_compose_project in tools/mg; t_dev_image checks they agree.
PROJECT="meguri-$(basename "$SB" | tr 'A-Z' 'a-z' | tr -c 'a-z0-9\n' '-')-$(printf '%s' "$SB" | cksum | cut -d' ' -f1)"

setup_sandbox() {
  mkdir -p "$SB/core"
  cp -R "$REPO/tools" "$SB/tools"
  cp -R "$REPO/docker" "$SB/docker"
  cp "$REPO/core/rust-toolchain.toml" "$SB/core/"
  cp "$REPO/.gitignore" "$SB/"
  git -C "$SB" init -q &&
    git -C "$SB" add -A &&
    git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm sandbox
}

cleanup() {
  docker compose -p "$PROJECT" -f "$SB/docker/compose.yml" down >/dev/null 2>&1
  docker image rm -f "$PROJECT-dev" "$PROJECT-fuzz" >/dev/null 2>&1
  docker volume rm -f "${PROJECT}_cargo-target" >/dev/null 2>&1
}
trap cleanup EXIT

# mg <args>: run the sandbox dispatcher with macOS /bin/bash. Sets RC, OUT
# (stdout), RESULT (last stdout line) and records RESULT for schema validation.
mg() {
  OUT="$(/bin/bash "$MG" "$@" 2>>"$SB/stderr.log")"
  RC=$?
  RESULT="$(printf '%s\n' "$OUT" | tail -n 1)"
  [ -f "$RESULT" ] && RESULTS="$RESULTS ${RESULT#"$SB"/}"
  return 0
}

field() { jq -r "$@" "$RESULT"; }

expect() { # expect <description> <actual> <expected>
  if [ "$2" != "$3" ]; then
    echo "$1: got '$2', want '$3'"
    return 1
  fi
}

t_unknown_job() {
  mg nosuchjob
  expect "exit code" "$RC" 2 &&
    expect "stdout lines" "$(printf '%s\n' "$OUT" | wc -l | tr -d ' ')" 1 &&
    expect "status" "$(field .status)" error &&
    expect "known jobs include doctor" "$(field '.details.known_jobs | index("doctor") != null')" true
}

t_dry_run_every_job() {
  local listing name placement _desc n=0
  listing="$(/bin/bash "$MG" --list)" || return 1
  while IFS="$(printf '\t')" read -r name placement _desc; do
    mg "$name" --dry-run
    expect "$name --dry-run exit" "$RC" 0 &&
      expect "$name dry_run" "$(field .dry_run)" true &&
      expect "$name placement" "$(field .details.job.placement)" "$placement" || return 1
    n=$((n + 1))
  done <<EOF
$listing
EOF
  [ "$n" -ge 5 ] || { echo "only $n jobs listed"; return 1; }
}

t_doctor_on_this_mac() {
  mg doctor
  jq -c '{status, message, missing: [.user_steps[] | select(.state == "missing") | .id]}' "$RESULT"
  expect "exit code" "$RC" 0 &&
    case "$(field .status)" in pass | warn) true ;; *) echo "status $(field .status)"; false ;; esac &&
    expect "failed checks" "$(field '[.details.checks[] | select(.status == "fail")] | length')" 0 &&
    expect "xcode build recorded" "$(field '.details.environment.xcode_build | type')" string &&
    expect "user_steps listed" "$(field '.user_steps | length > 0')" true
}

t_core_test_runs_in_dev_container() {
  mg core-test
  expect "exit code" "$RC" 0 &&
    expect "status" "$(field .status)" pass &&
    expect "placement" "$(field .placement)" docker &&
    expect "service" "$(field .service)" dev &&
    expect "container" "$(field .details.environment.container)" dev &&
    expect "os" "$(field '.details.environment.os | startswith("Linux")')" true &&
    expect "total tests" "$(field .summary.total)" 0 &&
    expect "latest" "$(readlink "$SB/build/mg/core-test/latest")" "$(field .run_id)"
}

t_dev_image_rebuild_reuses_layers() {
  mg dev-image
  expect "first build exit" "$RC" 0 || return 1
  local first="$RESULT"
  expect "dev image tag" "$(field .details.images.dev.tag)" "$PROJECT-dev" || return 1
  mg dev-image
  expect "second build exit" "$RC" 0 &&
    expect "dev image id" "$(field .details.images.dev.id)" "$(jq -r .details.images.dev.id "$first")" &&
    expect "fuzz image id" "$(field .details.images.fuzz.id)" "$(jq -r .details.images.fuzz.id "$first")" &&
    expect "dev steps executed" "$(field .details.images.dev.executed)" 0 &&
    expect "fuzz steps executed" "$(field .details.images.fuzz.executed)" 0 &&
    expect "dev steps cached" "$(field '.details.images.dev.cached > 0')" true
}

t_results_validate_against_schema() {
  [ -n "$RESULTS" ] || { echo "no results collected"; return 1; }
  # shellcheck disable=SC2086 # RESULTS is a space-separated list of relative paths
  BUILDX_NO_DEFAULT_ATTESTATIONS=1 docker compose -p "$PROJECT" -f "$SB/docker/compose.yml" \
    run --rm -T --no-deps dev python3 tools/tests/validate_result.py $RESULTS </dev/null
}

TESTS="t_unknown_job t_dry_run_every_job t_doctor_on_this_mac t_core_test_runs_in_dev_container
t_dev_image_rebuild_reuses_layers t_results_validate_against_schema"

echo "1..$(echo "$TESTS" | wc -w | tr -d ' ')"
if ! setup_sandbox >"$SB/setup.log" 2>&1; then
  echo "Bail out! could not create the sandbox in $SB (see setup.log)"
  exit 1
fi
n=0
failed=0
for t in $TESTS; do
  n=$((n + 1))
  name="$(printf '%s' "${t#t_}" | tr '_' ' ')"
  if "$t" >"$SB/diag.txt" 2>&1; then
    echo "ok $n $name"
  else
    failed=$((failed + 1))
    echo "not ok $n $name"
    sed 's/^/# /' "$SB/diag.txt"
  fi
done
[ "$failed" -eq 0 ]
