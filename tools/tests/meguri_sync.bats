#!/usr/bin/env bats
# Host integrator runs these through tools/mg selftest.
# These are static boundary proofs; no production server or browser suite exists here.

bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  SB="$BATS_TEST_TMPDIR/repo"
  mkdir -p "$SB"
  cp -R "$REPO_ROOT/tools" "$REPO_ROOT/docker" "$SB/"
  cp "$REPO_ROOT/.gitignore" "$SB/"
  git -C "$SB" init -q
  git -C "$SB" add -A
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm template
}

@test "public sync hosts are refused in production Rust and web sources, including retained commits" {
  mkdir -p "$SB/server/src" "$SB/server/web/src"
  printf 'const HOST: &str = "%s.%s";\n' ankiweb net >"$SB/server/src/lib.rs"
  printf 'export const host = "sync.%s.%s";\n' ankiweb net >"$SB/server/web/src/endpoint.ts"
  run "$SB/tools/gate/static-checks/ankiweb-hosts.sh" "$SB"
  [ "$status" -eq 1 ]
  [[ "$output" == *"server/src/lib.rs"* ]]
  [[ "$output" == *"server/web/src/endpoint.ts"* ]]
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  run "$SB/tools/gate/static-checks/ankiweb-hosts.sh" "$SB" HEAD
  [ "$status" -eq 1 ]
  [[ "$output" == *"server/src/lib.rs"* ]]
  [[ "$output" == *"server/web/src/endpoint.ts"* ]]
}

@test "static host scope preserves generated local test endpoints" {
  mkdir -p "$SB/server/tests" "$SB/server/web"
  printf 'const ENDPOINT: &str = "http://127.0.0.1:43210";\n' >"$SB/server/tests/endpoint.rs"
  printf 'export default { use: { baseURL: `http://localhost:43210` } };\n' >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/ankiweb-hosts.sh" "$SB"
  [ "$status" -eq 0 ]
}

@test "browser configs reject positive, conditional, duplicate and multiline repetition, but allow zero" {
  mkdir -p "$SB/server/web"
  local prefix=ret suffix=ries value
  for value in '2' 'process.env.CI ? 2 : 0' '{ runMode: 2 }'; do
    printf 'export default { %s%s: %s };\n' "$prefix" "$suffix" "$value" >"$SB/server/web/playwright.config.ts"
    run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
    [ "$status" -eq 1 ]
    [[ "$output" == *"server/web/playwright.config.ts"* ]]
  done
  printf 'export default { %s%s: 2, %s%s: 0 };\n' "$prefix" "$suffix" "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
  printf 'export default {\n %s%s\n : 2\n};\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
  printf 'export default { "%s%s": /* disabled */ 0 };\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  printf 'export default { %s%s: 2 };\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  printf 'export default { %s%s: 0 };\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 1 ]
}

@test "Vitest singular repetition requires literal zero in the working tree and retained tree" {
  mkdir -p "$SB/server/web"
  local prefix=ret suffix=ry value
  for value in '2' 'process.env.CI ? 2 : 0' 'false' '"0"' '0 + 2'; do
    printf 'export default { test: { %s%s: %s } };\n' "$prefix" "$suffix" "$value" >"$SB/server/web/vitest.config.ts"
    run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
    [ "$status" -eq 1 ]
    [[ "$output" == *"server/web/vitest.config.ts"* ]]
  done
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  printf 'export default { test: { "%s%s": /* disabled */ 0 } };\n' "$prefix" "$suffix" >"$SB/server/web/vitest.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 1 ]
}

@test "hex and Unicode escaped browser option names cannot hide repetition" {
  mkdir -p "$SB/server/web"
  local key value
  for key in '"re\x74ry"' "'re\x74ries'" '"re\u0074ry"' '"re\u{74}ries"' '"\x72e\u{74}ry"'; do
    for value in 2 0; do
      printf 'export default {\n  %s: %s\n};\n' "$key" "$value" >"$SB/server/web/vitest.config.ts"
      run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
      if [ "$value" -eq 0 ]; then
        [ "$status" -eq 0 ]
      else
        [ "$status" -eq 1 ]
        [[ "$output" == *'"line": 2'* ]]
      fi
    done
  done
  printf '%s\n' 'export default { "re\x74ry": 2 };' >"$SB/server/web/vitest.config.ts"
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  printf '%s\n' 'export default { "re\x74ry": 0 };' >"$SB/server/web/vitest.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 1 ]
}

@test "unsupported escaped properties and escaped identifiers fail closed" {
  mkdir -p "$SB/server/web"
  local key
  for key in '"\162etry"' '"re\qtry"' '"re\u{110000}try"' 're\u0074ry' '["re\x74ry"]'; do
    printf 'export default { %s: 0 };\n' "$key" >"$SB/server/web/vitest.config.ts"
    run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
    [ "$status" -eq 1 ]
  done
  # JS also allows line continuation inside strings; require review rather
  # than silently treating the fragments as an unrelated property.
  cat >"$SB/server/web/vitest.config.ts" <<'JS'
export default { "re\
try": 2 };
JS
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
}

@test "computed browser properties fail closed, including parenthesized escaped keys in retained trees" {
  mkdir -p "$SB/server/web"
  local key value
  for key in '[("re\x74ry")]' '[(("re\u0074ry"))]' '["re" + "try"]' '[option]' '[(`retry`)]'; do
    for value in 2 0; do
      printf 'export default { test: {\n  %s: %s\n} };\n' "$key" "$value" >"$SB/server/web/vitest.config.ts"
      run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
      [ "$status" -eq 1 ]
      [[ "$output" == *"computed property syntax needs review"* ]]
      [[ "$output" == *'"line": 2'* ]]
    done
  done
  printf '%s\n' 'export default { test: { [("re\x74ry")]: 2 } };' >"$SB/server/web/vitest.config.ts"
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  printf '%s\n' 'export default { test: { "re\x74ry": 0 } };' >"$SB/server/web/vitest.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 1 ]
  [[ "$output" == *"computed property syntax needs review"* ]]
}

@test "all automatic Vite and Vitest config extensions enforce positive and zero values in both trees" {
  mkdir -p "$SB/server/web"
  local runner extension
  for runner in vite vitest; do
    for extension in ts mts cts js mjs cjs; do
      printf 'export default { test: { retry: 0 } };\n' >"$SB/server/web/$runner.config.$extension"
    done
  done
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm zero-fixtures
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 0 ]
  for runner in vite vitest; do
    for extension in ts mts cts js mjs cjs; do
      printf 'export default { test: { retry: 2 } };\n' >"$SB/server/web/$runner.config.$extension"
    done
  done
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
  for runner in vite vitest; do
    for extension in ts mts cts js mjs cjs; do
      [[ "$output" == *"server/web/$runner.config.$extension"* ]]
    done
  done
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm positive-fixtures
  for runner in vite vitest; do
    for extension in ts mts cts js mjs cjs; do
      printf 'export default { test: { retry: 0 } };\n' >"$SB/server/web/$runner.config.$extension"
    done
  done
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 1 ]
  for runner in vite vitest; do
    for extension in ts mts cts js mjs cjs; do
      [[ "$output" == *"server/web/$runner.config.$extension"* ]]
    done
  done
}

@test "browser config paths preserve spaces and newlines with exact attribution in both trees" {
  local cfg=$'server/web/space dir\nnested/vite.config.ts'
  mkdir -p "$SB/${cfg%/*}"
  printf 'export default { test: {\n  retry: 0\n} };\n' >"$SB/$cfg"
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm zero-fixture
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 0 ]
  printf 'export default { test: {\n  retry: 2\n} };\n' >"$SB/$cfg"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
  printf '%s\n' "$output" | jq -e --arg path "$cfg" 'select(.path == $path and .line == 2)'
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm positive-fixture
  printf 'export default { test: { retry: 0 } };\n' >"$SB/$cfg"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 1 ]
  printf '%s\n' "$output" | jq -e --arg path "$cfg" 'select(.path == $path and .line == 2)'
}

@test "missing and unreadable browser config content cannot become an empty passing scan" {
  local cfg="$SB/server/web/vitest.config.ts"
  mkdir -p "${cfg%/*}"
  printf 'export default { test: { retry: 0 } };\n' >"$cfg"
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  rm "$cfg"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 2 ]
  [[ "$output" == *"could not read browser configs"* ]]
  # A directory in place of tracked content fails even for a root test runner;
  # chmod alone would not reliably demonstrate an unreadable file there.
  mkdir "$cfg"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 2 ]
  [[ "$output" == *"could not read browser configs"* ]]
  rmdir "$cfg"
  printf '\377' >"$cfg"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 2 ]
  [[ "$output" == *"could not read browser configs"* ]]
  printf 'export default { test: { retry: 0 } };\n' >"$cfg"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
}

@test "browser enumeration and retained content reader failures are checked" {
  mkdir -p "$SB/server/web"
  printf 'export default { test: { retry: 0 } };\n' >"$SB/server/web/vite.config.ts"
  git -C "$SB" add server
  git -C "$SB" -c user.name=selftest -c user.email=selftest@localhost commit -qm fixture
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 0 ]
  local fakebin="$BATS_TEST_TMPDIR/fakebin" real_git operation
  real_git="$(command -v git)"
  mkdir -p "$fakebin"
  cat >"$fakebin/git" <<'SH'
#!/bin/bash
if [ "${3:-}" = "$BROWSER_TEST_FAIL" ]; then
  echo 'synthetic browser Git read failure' >&2
  exit 73
fi
exec "$BROWSER_TEST_REAL_GIT" "$@"
SH
  chmod +x "$fakebin/git"
  for operation in ls-files ls-tree show; do
    local args=("$SB")
    [ "$operation" = ls-files ] || args+=(HEAD)
    run env PATH="$fakebin:$PATH" BROWSER_TEST_REAL_GIT="$real_git" BROWSER_TEST_FAIL="$operation" \
      "$SB/tools/gate/static-checks/no-retries.sh" "${args[@]}"
    [ "$status" -eq 2 ]
    [[ "$output" == *"could not read browser configs"* ]]
  done
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB" HEAD
  [ "$status" -eq 0 ]
}

@test "template URL strings do not hide browser repetition and unsupported templates fail closed" {
  mkdir -p "$SB/server/web"
  local prefix=ret suffix=ries value
  for value in 2 0; do
    printf 'export default { use: { baseURL: `http://localhost:3000` }, %s%s: %s };\n' "$prefix" "$suffix" "$value" >"$SB/server/web/playwright.config.ts"
    run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
    if [ "$value" -eq 0 ]; then
      [ "$status" -eq 0 ]
    else
      [ "$status" -eq 1 ]
      [[ "$output" == *"server/web/playwright.config.ts"* ]]
    fi
  done
  # Escaped delimiters/comment markers in plain templates are still strings.
  printf 'export default { label: `a\`b // /* text`, %s%s: 0 };\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 0 ]
  printf 'export default { use: { baseURL: `${process.env.URL}` }, %s%s: 0 };\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
  printf 'export default { [`%s%s`]: 2 };\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
  printf 'export default { use: { baseURL: `http://localhost:3000 }, %s%s: 2 };\n' "$prefix" "$suffix" >"$SB/server/web/playwright.config.ts"
  run "$SB/tools/gate/static-checks/no-retries.sh" "$SB"
  [ "$status" -eq 1 ]
}
