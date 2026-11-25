#!/usr/bin/env bats
# core-lint on a copy of the real workspace: what it must reject. Each
# tools/mg core-lint here compiles the workspace (clippy, layering-check, the
# feature-free meguri-sync build), and the file's tests share one target
# directory, so only the first builds the third-party crates. These tests and
# core_lint_allows.bats were one section of mg_selftest.bats; they are two
# files so that the two chains of builds run side by side.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

. "$REPO_ROOT/tools/tests/mg_sandbox.sh"

@test "core-lint fails naming the forbidden edges: tokio in sched, sched in search, stats in package" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  vendor_crate tokio MIT
  add_dep crates/meguri-sched 'tokio = { path = "../../../vendor/tokio" }'
  # An upward edge to a crate that does not depend on the source: since U40
  # meguri-engine depends on meguri-sched, so sched -> engine is a cycle that
  # cargo refuses before layering-check runs.
  add_dep crates/meguri-search 'meguri-sched.workspace = true'
  add_dep crates/meguri-package 'meguri-stats.workspace = true'
  mg core-lint
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  # Preserve actionable compiler diagnostics if the deliberately changed
  # graph also exposes a fixture build error.
  jq -r '.failures[] | "\(.id): \(.message)"' "$result" >&2
  [ "$(failure_ids)" = "layering/edge/meguri-package -> meguri-stats
layering/edge/meguri-search -> meguri-sched
layering/restricted/meguri-sched -> tokio" ]
  local m
  m="$(jq -r '.failures[] | select(.id == "layering/restricted/meguri-sched -> tokio") | .message' "$result")"
  [[ "$m" == *"meguri-sched depends on tokio"* ]]
  m="$(jq -r '.failures[] | select(.id == "layering/edge/meguri-search -> meguri-sched") | .message' "$result")"
  [[ "$m" == *"edges.toml row 5"*"(row 6)"* ]]
  m="$(jq -r '.failures[] | select(.id == "layering/edge/meguri-package -> meguri-stats") | .message' "$result")"
  [[ "$m" == *"edges.toml row 8"*"(row 8)"*siblings* ]]
  [ "$(field '.failures[0].repro')" = "tools/mg core-lint" ]
  [ "$(field -c .summary)" = '{"total":3,"passed":2,"failed":1,"skipped":0}' ]
  validate "$result"
}

@test "core-lint reports SystemTime::now outside the clock module while allowing unformatted code" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  printf '%s\n' '' 'pub fn stamp() -> std::time::SystemTime {' '    std::time::SystemTime::now()' '}' \
    >>"$SB/core/crates/meguri-text/src/lib.rs"
  printf '%s\n' 'pub fn  spaced ( ) { }' >>"$SB/core/crates/meguri-search/src/lib.rs"
  mg core-lint
  [ "$status" -eq 1 ]
  local ids
  ids="$(failure_ids)"
  [[ "$ids" == *"clippy/clippy::disallowed_methods/crates/meguri-text/src/lib.rs:"* ]]
  [ "$(field '.failures | length')" -eq 1 ]
  [[ "$(jq -r '.failures[] | select(.id | startswith("clippy/")) | .message' "$result")" == *"std::time::SystemTime::now"* ]]
  validate "$result"
}

@test "core-lint still denies the four reachable rand paths with allow-invalid" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  # Compile this public probe without calling it or executing randomness.
  printf '%s\n' '' '#[allow(deprecated)]' 'pub fn forbidden_randomness_probe() {' \
    '    use rand::SeedableRng;' \
    '' \
    '    let _ = rand::thread_rng();' \
    '    let _: Option<rand::rngs::OsRng> = None;' \
    '    let _ = rand::rngs::StdRng::from_os_rng();' \
    '    let _ = rand::rngs::StdRng::try_from_os_rng();' \
    '}' >>"$SB/core/crates/meguri-engine/src/lib.rs"
  mg core-lint
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  local path
  for path in rand::thread_rng rand::SeedableRng::from_os_rng rand::SeedableRng::try_from_os_rng; do
    jq -e --arg path "$path" '.failures | any(
      (.id | startswith("clippy/clippy::disallowed_methods/crates/meguri-engine/src/lib.rs:"))
      and (.message | contains("use of a disallowed method `" + $path + "`")))' "$result"
  done
  jq -e '.failures | any(
    (.id | startswith("clippy/clippy::disallowed_types/crates/meguri-engine/src/lib.rs:"))
    and (.message | contains("use of a disallowed type `rand::rngs::OsRng`")))' "$result"
  validate "$result"
}
