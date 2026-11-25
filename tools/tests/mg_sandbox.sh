# The sandbox of the tools/mg self-tests, sourced by mg_selftest.bats and the
# core_lint_*.bats files after they set REPO_ROOT: each test gets a fresh copy
# of tools/ and docker/ in a throwaway git repo, so its results never land in
# the real build/ directory.

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
  SB="$BATS_TEST_TMPDIR/repo"
  cp -R "$BATS_FILE_TMPDIR/template" "$SB"
  export CARGO_TARGET_DIR="$BATS_TEST_TMPDIR/target" CARGO_NET_OFFLINE=true
}

# mg <args>: run the sandbox's tools/mg. Sets status, output (stdout), stderr,
# lines, and result (the last stdout line).
mg() {
  run --separate-stderr "$SB/tools/mg" "$@"
  result="${lines[${#lines[@]} - 1]:-}"
}

field() { jq -r "$@" "$result"; }

validate() { python3 "$SB/tools/tests/validate_result.py" "$@"; }

write_file() { # write_file <path under the sandbox> <contents>
  mkdir -p "$(dirname "$SB/$1")"
  printf '%s\n' "$2" >"$SB/$1"
}

# copy_core: the real core/ workspace (without build output) in the sandbox,
# with its crates.io dependencies in CARGO_HOME. Fetching is a no-op once the
# shared cargo home has them. Builds share one target directory per file.
copy_core() {
  rm -rf "$SB/core"
  (cd "$REPO_ROOT" && tar cf - --exclude=core/target core) | (cd "$SB" && tar xf -)
  # Rust tests embed neutral conformance goldens with include_str!.
  mkdir -p "$SB/conformance"
  cp -R "$REPO_ROOT/conformance/goldens" "$SB/conformance/"
  # The Unicode10 unit test embeds this independent neutral reference.
  mkdir -p "$SB/specs/render"
  cp "$REPO_ROOT/specs/render/unicode10-marks.json" "$SB/specs/render/"
  CARGO_NET_OFFLINE=false cargo fetch --locked --manifest-path "$SB/core/Cargo.toml" >&2
  export CARGO_TARGET_DIR="$BATS_FILE_TMPDIR/core-target"
}

# That target is several GB once a file has built the real workspace, and bats
# keeps a file's directory until the whole run ends: drop it with the file.
teardown_file() { rm -rf "$BATS_FILE_TMPDIR/core-target"; }

# vendor_crate <name> <license>: a stand-in for a crates.io crate, outside the workspace.
vendor_crate() {
  write_file "vendor/$1/Cargo.toml" "[package]
name = \"$1\"
version = \"0.1.0\"
edition = \"2024\"
license = \"$2\""
  write_file "vendor/$1/src/lib.rs" ""
}

# add_dep <crate dir under core/> <dependency line>: under [dependencies].
add_dep() {
  local manifest="$SB/core/$1/Cargo.toml"
  if grep -q '^\[dependencies\]$' "$manifest"; then
    sed -i "/^\[dependencies\]\$/a $2" "$manifest"
  else
    printf '\n[dependencies]\n%s\n' "$2" >>"$manifest"
  fi
}

failure_ids() { jq -r '[.failures[].id] | sort | join("\n")' "$result"; }
