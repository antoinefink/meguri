#!/usr/bin/env bats
# core-lint on a copy of the real workspace: what it must let through. See
# core_lint_rejects.bats for why these are a file of their own.

bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

. "$REPO_ROOT/tools/tests/mg_sandbox.sh"

@test "core-lint passes all three retained checks with formatting-only differences" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  printf '%s\n' '' 'pub fn  spaced ( ) { }' >>"$SB/core/crates/meguri-search/src/lib.rs"
  mg core-lint
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field '.failures | length')" -eq 0 ]
  [ "$(field -c .summary)" = '{"total":3,"passed":3,"failed":0,"skipped":0}' ]
  validate "$result"
}

@test "core-lint allows the three delivery style rules but still rejects forbidden time" {
  [ "${MG_CONTAINER:-}" = dev ] || skip "needs the dev container"
  copy_core
  cat >>"$SB/core/crates/meguri-text/src/lib.rs" <<'EOF'

pub fn delivery_collapsible_if(outer: bool, inner: bool, observed: &mut u8) {
    if outer {
        if inner {
            *observed = 1;
        }
    }
}

pub fn delivery_manual_inspect(value: Option<u8>, observed: &mut u8) -> Option<u8> {
    value.map(|item| {
        *observed = item;
        item
    })
}

pub fn delivery_many_arguments(
    a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8,
) -> [u8; 8] {
    [a, b, c, d, e, f, g, h]
}
EOF
  mg core-lint
  [ "$status" -eq 0 ]
  [ "$(field .status)" = pass ]
  [ "$(field '.failures | length')" -eq 0 ]
  [ "$(field -c .summary)" = '{"total":3,"passed":3,"failed":0,"skipped":0}' ]
  validate "$result"

  # A changed fixture must still fail on a real clock violation alongside the style forms.
  printf '%s\n' '' 'pub fn delivery_forbidden_clock() -> std::time::SystemTime {' \
    '    std::time::SystemTime::now()' '}' >>"$SB/core/crates/meguri-text/src/lib.rs"
  mg core-lint
  [ "$status" -eq 1 ]
  [ "$(field .status)" = fail ]
  [ "$(field '.failures | length')" -eq 1 ]
  [[ "$(failure_ids)" == "clippy/clippy::disallowed_methods/crates/meguri-text/src/lib.rs:"* ]]
  [[ "$(field '.failures[0].message')" == *"std::time::SystemTime::now"* ]]
  [ "$(field -c .summary)" = '{"total":3,"passed":2,"failed":1,"skipped":0}' ]
  validate "$result"
}
