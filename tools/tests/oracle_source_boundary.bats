#!/usr/bin/env bats

setup() {
  local root
  root="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
  # Exercise the real host check without running its other oracle checks.
  source <(sed -n '/^t_no_meguri_code_refers_to_oracle()/,/^}/p' "$root/tools/tests/oracle_host_checks.sh")
  export REPO="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$REPO/core" "$REPO/app"
  git -C "$REPO" init -q
  printf '.build/\ntarget/\n' >"$REPO/.gitignore"
  git -C "$REPO" add .gitignore
  git -C "$REPO" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
}

@test "oracle source boundary ignores generated compilation caches" {
  mkdir -p "$REPO/app/Packages/Kit/.build/CompilationCache.noindex"
  printf '%s\n' '../oracle/driver' >"$REPO/app/Packages/Kit/.build/CompilationCache.noindex/data.v1"
  run t_no_meguri_code_refers_to_oracle
  [ "$status" -eq 0 ]
}

@test "oracle source boundary rejects tracked references without printing source" {
  printf '%s\n' 'let sourceBoundaryProbe = "../oracle/driver";' >"$REPO/core/source.rs"
  git -C "$REPO" add core/source.rs
  run t_no_meguri_code_refers_to_oracle
  [ "$status" -ne 0 ]
  [[ "$output" == *core/source.rs* ]]
  [[ "$output" != *sourceBoundaryProbe* ]]
}

@test "oracle source boundary rejects new untracked source references" {
  printf '%s\n' '../oracle/entrypoint' >"$REPO/app/Source.swift"
  run t_no_meguri_code_refers_to_oracle
  [ "$status" -ne 0 ]
  [[ "$output" == *app/Source.swift* ]]
}

@test "oracle source boundary checks tracked source despite ignore rules" {
  mkdir -p "$REPO/core/target"
  printf '%s\n' '../oracle/requirements' >"$REPO/core/target/source.rs"
  git -C "$REPO" add -f core/target/source.rs
  run t_no_meguri_code_refers_to_oracle
  [ "$status" -ne 0 ]
}

@test "oracle source boundary fails closed when Git cannot scan" {
  REPO="$BATS_TEST_TMPDIR/missing"
  run t_no_meguri_code_refers_to_oracle
  [ "$status" -ne 0 ]
}

@test "oracle source boundary accepts source without oracle references" {
  printf '%s\n' 'pub struct Collection;' >"$REPO/core/source.rs"
  printf '%s\n' 'struct HomeView {}' >"$REPO/app/Source.swift"
  git -C "$REPO" add core/source.rs
  run t_no_meguri_code_refers_to_oracle
  [ "$status" -eq 0 ]
}
