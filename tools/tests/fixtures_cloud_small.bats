#!/usr/bin/env bats
# Wrapper evidence only. Real collection/readback proof is the Rust suite.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup_file() {
  local t="$BATS_FILE_TMPDIR/template"
  mkdir -p "$t/core"
  cp -R "$REPO_ROOT/tools" "$t/tools"
  cp -R "$REPO_ROOT/docker" "$t/docker"
  cp "$REPO_ROOT/core/rust-toolchain.toml" "$t/core/"
  cp "$REPO_ROOT/.gitignore" "$t/"
  git -C "$t" init -q
  git -C "$t" add -A
  git -C "$t" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

setup() {
  export SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  local stubs="$BATS_TEST_TMPDIR/stubs" target="$BATS_TEST_TMPDIR/target"
  local testkit="$SB/core/crates/meguri-testkit"
  mkdir -p "$stubs" "$target/fixtures" "$testkit/profiles/worst-cases" "$testkit/fixtures/recipes"
  printf '{"format":1}\n' >"$testkit/profiles/user-shape.json"
  printf '{}\n' >"$testkit/profiles/worst-cases/wide.json"
  printf 'recipe: tiny\n' >"$testkit/fixtures/recipes/tiny.yaml"
  cat >"$stubs/cargo" <<STUB
#!/bin/bash
echo "cargo \$*" >>"\$STUB_LOG"
case "\$1" in
  build) exit "\${STUB_BUILD_EXIT:-0}" ;;
  metadata) printf '{"target_directory":"%s"}\n' "$target" ;;
  *) exit 97 ;;
esac
STUB
  cat >"$target/fixtures/meguri-cli" <<'STUB'
#!/bin/bash
# This is intentionally a receipt stub, never generated-row evidence.
echo "meguri-cli $*" >>"$STUB_LOG"
[ "${STUB_CLI_EXIT:-0}" = 0 ] || exit "$STUB_CLI_EXIT"
target="$2" out="" seed=1
while [ "$#" -gt 0 ]; do
  case "$1" in
    --out) out="$2"; shift 2 ;;
    --seed) seed="$2"; shift 2 ;;
    *) shift ;;
  esac
done
cached=false
[ ! -f "$out/manifest.json" ] || cached=true
mkdir -p "$out"
if [ "$target" != cloud-small ]; then
  printf '{}\n' >"$out/manifest.json"
  jq -nc --arg dir "$out" --argjson cached "$cached" \
    '{dir:$dir,cached:$cached,elapsed_ms:1,peak_rss_kb:1,manifest:{counts:{cards:1}}}'
  exit
fi
jq -nc --arg dir "$out" --argjson seed "$seed" --argjson cached "$cached" \
  --argjson elapsed "${STUB_ELAPSED_MS:-1}" --argjson peak "${STUB_PEAK_KB:-1}" '
  {notes:20,cards:20,revlog:60,notetypes:1,decks:2,presets:1,tags:20,media_refs:5,media_files:5,media_bytes:51200} as $counts
  | ("a" * 64) as $hash
  | {dir:$dir,cached:$cached,elapsed_ms:$elapsed,peak_rss_kb:$peak,
     request:{seed:$seed,now_ms:1735689600000,utc_offset_secs:0,rollover_hour:4,notes:20,cards:20,
              media:"files",media_files:5,media_file_bytes:10240,reviews_per_card:3,
              review_days_before_pinned:[3,2,1],answer_seconds_after_day_start:28800,answer_ordinal_seconds:true},
     manifest:{kind:"cloud-small",name:"cloud-small",seed:$seed,now_ms:1735689600000,utc_offset_secs:0,
               cards_requested:20,media:"files",counts:$counts,dump_sha256:$hash},
     observed:{counts:$counts,dump_sha256:$hash}}' >"$out/receipt-stub.json"
case "${STUB_BAD_RECEIPT:-}" in
  missing) filter='del(.observed)' ;;
  counts) filter='.observed.counts.cards=19' ;;
  clock) filter='.request.now_ms=1894701600000' ;;
  hash) filter='.observed.dump_sha256="wrong"' ;;
  *) filter='.' ;;
esac
jq '.manifest' "$out/receipt-stub.json" >"$out/manifest.json"
jq "$filter" "$out/receipt-stub.json"
STUB
  chmod +x "$stubs/cargo" "$target/fixtures/meguri-cli"
  export PATH="$stubs:$PATH" MG_CONTAINER=dev STUB_LOG="$BATS_TEST_TMPDIR/commands.log"
  export CARGO_TARGET_DIR="$target" CARGO_NET_OFFLINE=true
  : >"$STUB_LOG"
}

mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

@test "cloud-small selects the dedicated generator and declares its request/readback and manifest" {
  mg fixtures cloud-small --seed 37
  [ "$status" -eq 0 ]
  jq -e --arg fixture "$SB/build/fixtures/cloud-small-37" '.status=="pass" and .summary.passed==1
    and .details.fixtures[0].kind=="cloud-small"
    and .details.fixtures[0].dir=="build/fixtures/cloud-small-37"
    and .details.fixtures[0].request.seed==37
    and .details.fixtures[0].observed.counts.revlog==60
    and ([.artifacts[] | select(.kind=="fixture" and .path==$fixture)] | length)==1
    and ([.artifacts[] | select(.kind=="generation")] | length)==1
    and ([.artifacts[] | select(.kind=="manifest")] | length)==1' "$result"
  grep -qx 'meguri-cli generate cloud-small --seed 37 --out build/fixtures/cloud-small-37 --cache' "$STUB_LOG"
  local receipt="$(jq -r '.artifacts[] | select(.kind=="generation") | .path' "$result")"
  case "$receipt" in /*) ;; *) receipt="$SB/$receipt" ;; esac
  jq -e '.request.seed==37 and .observed.counts==.manifest.counts' "$receipt"
  [ -f "$SB/build/fixtures/cloud-small-37/manifest.json" ]
  python3 "$SB/tools/tests/validate_result.py" "$result"
  mg fixtures cloud-small --seed 37 --media files --cards 20
  [ "$status" -eq 0 ]
  jq -e '.details.fixtures[0].cached==true' "$result"
  grep -qx 'meguri-cli generate cloud-small --cards 20 --media files --seed 37 --out build/fixtures/cloud-small-37 --cache' "$STUB_LOG"
}

@test "cloud-small rejects incompatible overrides before building and keeps no-target usage" {
  local args
  for args in '' 'cloud-small --media references' 'cloud-small --cards 21' \
    'cloud-small --now 2030-01-15T10:00:00Z' 'cloud-small --profile other.json' \
    'cloud-small --seed bad' '10k cloud-small --media references'; do
    # shellcheck disable=SC2086 # intentional word lists, no paths with spaces
    mg fixtures $args
    [ "$status" -eq 2 ]
    jq -e '.status=="error" and .failures[0].id=="usage"' "$result"
  done
  [ ! -s "$STUB_LOG" ]
}

@test "cloud-small coexists with the unchanged size worst-case and recipe classification" {
  mg fixtures cloud-small 10k worst-cases recipes --seed 3
  [ "$status" -eq 0 ]
  jq -e '.summary.total==4 and [.details.fixtures[].kind]==["cloud-small","size","worst-case","recipe"]' "$result"
  local shape="$(sha256sum <"$SB/core/crates/meguri-testkit/profiles/user-shape.json" | cut -c1-16)"
  grep -qx "meguri-cli generate 10k --seed 3 --out build/fixtures/$shape-3-10k --cache" "$STUB_LOG"
  grep -qx "meguri-cli generate worst-case wide --seed 3 --out build/fixtures/$shape-3-wide --cache" "$STUB_LOG"
  grep -qx 'meguri-cli generate recipe tiny --seed 3 --out build/fixtures/recipes/tiny-3 --cache' "$STUB_LOG"
}

@test "cloud-small fails missing or inconsistent observed receipts" {
  local fault
  for fault in missing counts clock hash; do
    STUB_BAD_RECEIPT="$fault" mg fixtures cloud-small --seed 8
    [ "$status" -eq 1 ]
    jq -e '.status=="fail" and .failures[0].id=="fixtures/cloud-small/receipt"
      and .failures[0].repro=="tools/mg fixtures cloud-small --seed 8"
      and ([.artifacts[] | select(.kind=="fixture")] | length)==0' "$result"
  done
}

@test "cloud-small preserves generator build time and memory failures with repro" {
  STUB_CLI_EXIT=1 mg fixtures cloud-small --seed 9
  [ "$status" -eq 1 ]
  jq -e '.failures[0].id=="fixtures/cloud-small" and .failures[0].repro=="tools/mg fixtures cloud-small --seed 9"' "$result"
  STUB_BUILD_EXIT=101 mg fixtures cloud-small
  [ "$status" -eq 1 ]
  jq -e '.failures[0].id=="fixtures-build"' "$result"
  STUB_ELAPSED_MS=600001 mg fixtures cloud-small
  [ "$status" -eq 1 ]
  jq -e '.failures[0].id=="fixtures/cloud-small/time"' "$result"
  rm -rf "$SB/build/fixtures/cloud-small-1"
  STUB_PEAK_KB=2097153 mg fixtures cloud-small
  [ "$status" -eq 1 ]
  jq -e '.failures[0].id=="fixtures/cloud-small/memory"' "$result"
}
