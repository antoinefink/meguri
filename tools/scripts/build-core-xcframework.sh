#!/bin/bash
# Builds the Rust core (core/crates/meguri-ffi) into the static-library
# XCFramework and the Swift bindings of app/Packages/MeguriCore (KTD9, U6).
# Called by `tools/mg core-xcframework` and by the Xcode scheme pre-action.
#
#   build-core-xcframework.sh [--configuration Debug|Profile|Release|AppStore]
#                             [--slices mac,sim,device] [--report FILE]
#   build-core-xcframework.sh --xcode [--report FILE]
#
# --configuration  Xcode configuration (default Debug). It picks the cargo
#                  profile and the Rust features (KTD18, KTD21):
#                    Debug     dev      test-hooks,ankiweb
#                    Profile   release  test-hooks,ankiweb
#                    Release   release  ankiweb
#                    AppStore  release  (none)
# --slices         Exactly the slices to put in the XCFramework:
#                  mac = aarch64-apple-darwin, sim = aarch64-apple-ios-sim,
#                  device = aarch64-apple-ios. mac is always built: the
#                  bindings are generated from it. Without --slices: mac,sim
#                  plus any slice already built for this configuration.
# --xcode          Scheme pre-action mode: the configuration comes from
#                  $CONFIGURATION and the slice from $PLATFORM_NAME. The slice
#                  is added to the ones already built for that configuration,
#                  so switching destinations never drops a slice.
# --report FILE    Write a JSON summary of the run to FILE.
#
# Outputs (gitignored):
#   app/Packages/MeguriCore/Binaries/MeguriCoreFFI.xcframework
#   app/Packages/MeguriCore/Binaries/.stamp
#   app/Packages/MeguriCore/Sources/MeguriCoreBindings/MeguriCoreBindings.swift
#
# A run hashes the content of every non-ignored file under core/, this
# script, its Swift reader repair, the configuration and the slice list.
# When the hash equals the stamp and the outputs exist, it exits without
# invoking cargo. Otherwise it
# takes the build locks, builds each slice, checks that every slice exports
# the same uniffi_* symbols and yields the same bindings, generates the
# bindings from the Mac slice, creates the XCFramework in a temporary
# directory and replaces an output file only when its bytes differ, so Xcode
# and SwiftPM recompile and relink only what changed. Outputs and the stamp
# are untouched when a check fails; a failed or interrupted replace leaves a
# stamp that forces the next run to rebuild.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
SELF="$ROOT/tools/scripts/build-core-xcframework.sh"
PKG="$ROOT/app/Packages/MeguriCore"
XCF="$PKG/Binaries/MeguriCoreFFI.xcframework"
STAMP="$PKG/Binaries/.stamp"
OUTPUT_LOCK="$PKG/Binaries/.lock"
BINDINGS_DIR="$PKG/Sources/MeguriCoreBindings"
BINDINGS="$BINDINGS_DIR/MeguriCoreBindings.swift"
FFI_MODULE=MeguriCoreFFI
DEPLOYMENT_TARGET=27.0

die() {
  echo "build-core-xcframework: $*" >&2
  exit 1
}

usage() {
  sed -n '6,8p' "$SELF" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

configuration=Debug
slices_arg=""
xcode=false
report=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --configuration) configuration="${2:?}" && shift ;;
    --slices) slices_arg="${2:?}" && shift ;;
    --xcode) xcode=true ;;
    --report) report="${2:?}" && shift ;;
    -h | --help) usage ;;
    *) echo "build-core-xcframework: unknown argument '$1'" >&2 && usage ;;
  esac
  shift
done

# The stamp holds `key=value` lines: hash, configuration, profile, features, slices.
stamp_value() { [ -f "$STAMP" ] && sed -n "s/^$1=//p" "$STAMP" || true; }

slice_triple() {
  case "$1" in
    mac) echo aarch64-apple-darwin ;;
    sim) echo aarch64-apple-ios-sim ;;
    device) echo aarch64-apple-ios ;;
    *) die "unknown slice '$1' (expected mac, sim or device)" ;;
  esac
}

# The XCFramework library identifier xcodebuild gives each slice.
slice_dir() {
  case "$1" in
    mac) echo macos-arm64 ;;
    sim) echo ios-arm64-simulator ;;
    device) echo ios-arm64 ;;
  esac
}

if [ "$xcode" = true ]; then
  [ -z "$slices_arg" ] || die "--xcode derives the slices from \$PLATFORM_NAME; do not pass --slices"
  configuration="${CONFIGURATION:?--xcode needs \$CONFIGURATION from Xcode}"
  case "${PLATFORM_NAME:?--xcode needs \$PLATFORM_NAME from Xcode}" in
    macosx) slices_arg=mac ;;
    iphonesimulator) slices_arg=sim ;;
    iphoneos) slices_arg=device ;;
    *) die "unsupported PLATFORM_NAME '$PLATFORM_NAME'" ;;
  esac
fi

case "$configuration" in
  Debug) profile=dev features=ankiweb,test-hooks ;;
  Profile) profile=release features=ankiweb,test-hooks ;;
  Release) profile=release features=ankiweb ;;
  AppStore) profile=release features="" ;;
  *) die "unknown configuration '$configuration' (Debug, Profile, Release or AppStore)" ;;
esac
case "$profile" in
  dev) profile_dir=debug ;;
  *) profile_dir="$profile" ;;
esac
target_root="${CARGO_TARGET_DIR:-$ROOT/core/target}"
case "$target_root" in /*) ;; *) target_root="$ROOT/core/$target_root" ;; esac

# Sets $slices. Without an explicit --slices, keep the slices already built
# for this configuration, so a test run never drops the device slice and a
# pre-action only adds one.
resolve_slices() {
  local wanted="$slices_arg" s
  if [ "$xcode" = true ] || [ -z "$slices_arg" ]; then
    [ -n "$wanted" ] || wanted=mac,sim
    if [ "$(stamp_value configuration)" = "$configuration" ]; then
      wanted="$wanted,$(stamp_value slices)"
    fi
  fi
  slices="$(printf '%s\n' mac "$wanted" | tr ',' '\n' | sed '/^$/d' | sort -u | tr '\n' ',' | sed 's/,$//')"
  for s in $(printf '%s' "$slices" | tr ',' ' '); do slice_triple "$s" >/dev/null; done
}

write_report() { # write_report <status> [replaced files...]
  [ -n "$report" ] || return 0
  local status="$1"
  shift
  {
    printf '{"status":"%s","configuration":"%s","profile":"%s","features":"%s","slices":"%s","hash":"%s",' \
      "$status" "$configuration" "$profile" "$features" "$slices" "$hash"
    printf '"xcframework":"%s","bindings":"%s","replaced":[' "$XCF" "$BINDINGS"
    local first=true f
    for f in "$@"; do
      $first || printf ','
      first=false
      printf '"%s"' "$f"
    done
    printf ']}\n'
  } >"$report"
}

# --- Input hash -------------------------------------------------------------
# Sets $hash: every file under core/ that git tracks or would track (new
# files count before they are added), with its content id, plus this script,
# its reader repair and the build parameters. Paths stay relative to the repo root in git's
# quoting (non-ASCII names, tabs), which hash-object --stdin-paths unquotes.
# git hash-object hashes every file in one process; shasum is a perl script
# and too slow to start for the no-change budget, where each git start costs
# about 12 ms and md5 (for the manifest) about 3 ms. Call it as a plain
# command: set -e must stop the script on any failure inside.
compute_hash() {
  local inputs ids deleted_pid _
  # Tracked files deleted from the work tree have no content to hash.
  (git -C "$ROOT" ls-files -d -- core | LC_ALL=C sort >"$TMP/deleted") &
  deleted_pid=$!
  git -C "$ROOT" ls-files -co --exclude-standard --deduplicate -- core | LC_ALL=C sort >"$TMP/inputs"
  wait "$deleted_pid"
  if [ -s "$TMP/deleted" ]; then
    LC_ALL=C comm -23 "$TMP/inputs" "$TMP/deleted" >"$TMP/kept"
    mv "$TMP/kept" "$TMP/inputs"
  fi
  echo tools/scripts/build-core-xcframework.sh >>"$TMP/inputs"
  echo tools/scripts/repair-swift-integer-reader.py >>"$TMP/inputs"
  git -C "$ROOT" hash-object --stdin-paths <"$TMP/inputs" >"$TMP/ids"
  wc -l "$TMP/inputs" "$TMP/ids" >"$TMP/counts"
  { read -r inputs _ && read -r ids _; } <"$TMP/counts"
  [ "$ids" = "$inputs" ] || die "git hash-object returned $ids content ids for $inputs inputs"
  {
    printf 'configuration=%s\nprofile=%s\nfeatures=%s\nslices=%s\n' "$configuration" "$profile" "$features" "$slices"
    paste "$TMP/inputs" "$TMP/ids"
  } >"$TMP/manifest"
  hash="$(/sbin/md5 -q "$TMP/manifest")"
}

outputs_present() {
  [ -f "$XCF/Info.plist" ] && [ -f "$BINDINGS" ] || return 1
  local s dir
  for s in $(printf '%s' "$slices" | tr ',' ' '); do
    dir="$XCF/$(slice_dir "$s")"
    [ -f "$dir/libmeguri_ffi.a" ] && [ -f "$dir/Headers/$FFI_MODULE/$FFI_MODULE.h" ] &&
      [ -f "$dir/Headers/$FFI_MODULE/module.modulemap" ] || return 1
  done
}

exit_if_up_to_date() {
  [ "$(stamp_value hash)" = "$hash" ] && outputs_present || return 0
  echo "build-core-xcframework: up to date ($configuration; $slices)" >&2
  write_report up-to-date
  exit 0
}

TMP="$(mktemp -d "${TMPDIR:-/tmp}/meguri-xcf.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
resolve_slices
compute_hash
exit_if_up_to_date

# --- One run at a time ------------------------------------------------------
# Profile, Release and AppStore share cargo's release archives, and every run
# writes the same outputs: two runs must never interleave a build, its checks
# and its writes. One lock per output directory, one per cargo target
# directory, taken in that order. They are flock(2) locks on fds 8 and 9,
# released when this script exits; commands that may outlive it (cargo,
# xcodebuild, bindgen) run without those fds.
take_lock() { # take_lock <fd> <lock file>
  lockf -s -t 0 "$1" && return 0
  echo "build-core-xcframework: waiting for another run to release $2" >&2
  lockf -s "$1" || die "could not lock $2"
}
mkdir -p "$(dirname "$OUTPUT_LOCK")" "$target_root"
exec 8>>"$OUTPUT_LOCK"
take_lock 8 "$OUTPUT_LOCK"
exec 9>>"$target_root/.meguri-xcframework.lock"
take_lock 9 "$target_root/.meguri-xcframework.lock"

# A run that held the locks may have built these inputs or changed the
# stamp's slices.
resolve_slices
compute_hash
exit_if_up_to_date

# --- Build each slice -------------------------------------------------------
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || die "cargo not found; install rustup (tools/mg doctor)"

# Xcode puts its build settings in a pre-action's environment. An iOS
# SDKROOT or deployment target would leak into the host-side build scripts
# and proc macros, so cargo runs without them and with explicit targets.
cargo_env() {
  env -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET -u IPHONEOS_DEPLOYMENT_TARGET \
    -u TVOS_DEPLOYMENT_TARGET -u WATCHOS_DEPLOYMENT_TARGET -u XROS_DEPLOYMENT_TARGET \
    -u PLATFORM_NAME -u ARCHS -u CONFIGURATION "$@" 8>&- 9>&-
}

feature_args=(--no-default-features)
[ -z "$features" ] || feature_args+=(--features "$features")

# Every later step reads this run's copy of each archive (an APFS clone), not
# cargo's, which the next build of another configuration rewrites.
lib_of() { echo "$TMP/lib/$1/libmeguri_ffi.a"; }
for s in $(printf '%s' "$slices" | tr ',' ' '); do
  triple="$(slice_triple "$s")"
  echo "build-core-xcframework: cargo build $triple ($configuration: $profile${features:+, $features})" >&2
  (cd "$ROOT/core" && cargo_env MACOSX_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET" \
    IPHONEOS_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET" \
    cargo build --locked -p meguri-ffi --lib --target "$triple" --profile "$profile" "${feature_args[@]}") ||
    die "cargo build failed for $triple"
  mkdir -p "$TMP/lib/$s"
  cp -c "$target_root/$triple/$profile_dir/libmeguri_ffi.a" "$(lib_of "$s")" ||
    die "could not copy the $triple archive"
done

# --- Same exported surface on every slice (KTD9) ----------------------------
# Rust embeds LLVM bitcode in its objects; --no-llvm-bc makes Xcode's nm read
# the Mach-O symbol table instead of failing on bitcode from a newer LLVM.
uniffi_symbols() { # uniffi_symbols <library> <output file>
  nm --no-llvm-bc -gUj "$1" >"$TMP/nm.out" 2>"$TMP/nm.err" || {
    cat "$TMP/nm.err" >&2
    die "nm failed on $1"
  }
  { grep '^_uniffi_' "$TMP/nm.out" || true; } | LC_ALL=C sort -u >"$2"
}
uniffi_symbols "$(lib_of mac)" "$TMP/symbols-mac"
[ -s "$TMP/symbols-mac" ] || die "no uniffi_* symbols in the aarch64-apple-darwin archive"
mismatch=false
for s in $(printf '%s' "$slices" | tr ',' ' '); do
  [ "$s" != mac ] || continue
  uniffi_symbols "$(lib_of "$s")" "$TMP/symbols-$s"
  if ! cmp -s "$TMP/symbols-mac" "$TMP/symbols-$s"; then
    mismatch=true
    echo "build-core-xcframework: the $s slice exports a different uniffi_* symbol set than the mac slice:" >&2
    LC_ALL=C comm -23 "$TMP/symbols-mac" "$TMP/symbols-$s" | sed 's/^_/  only on mac: /' >&2
    LC_ALL=C comm -13 "$TMP/symbols-mac" "$TMP/symbols-$s" | sed "s/^_/  only on $s: /" >&2
  fi
done
[ "$mismatch" = false ] ||
  die "slices differ in their exports; gate implementations by platform, never exports (KTD9). Outputs left unchanged."

# --- Bindings from the Mac slice --------------------------------------------
(cd "$ROOT/core" && cargo_env cargo build --locked -q -p uniffi-bindgen-swift) || die "building uniffi-bindgen-swift failed"
bindgen="$target_root/debug/uniffi-bindgen-swift"
generate() { # generate <slice>: that slice's bindings in $TMP/gen-<slice>
  mkdir -p "$TMP/gen-$1"
  (cd "$ROOT/core" && "$bindgen" "$(lib_of "$1")" "$TMP/gen-$1" --swift-sources --headers --modulemap \
    --module-name "$FFI_MODULE" --modulemap-filename module.modulemap --metadata-no-deps 8>&- 9>&-) ||
    die "uniffi-bindgen-swift failed on the $1 slice"
  python3 "$ROOT/tools/scripts/repair-swift-integer-reader.py" "$TMP/gen-$1/MeguriCoreBindings.swift" 8>&- 9>&-
}
generate mac

# Equal symbol names are not enough: a cfg-gated signature keeps its names and
# changes only a checksum, a cfg-gated record field changes no symbol at all.
# Every slice must yield the bindings the app compiles against.
for s in $(printf '%s' "$slices" | tr ',' ' '); do
  [ "$s" != mac ] || continue
  generate "$s"
  if ! diff -ru "$TMP/gen-mac" "$TMP/gen-$s" >"$TMP/bindings-$s.diff"; then
    mismatch=true
    echo "build-core-xcframework: the $s slice yields different bindings than the mac slice:" >&2
    sed -n '1,60p' "$TMP/bindings-$s.diff" >&2
  fi
done
[ "$mismatch" = false ] ||
  die "slices differ in their exported API; gate implementations by platform, never signatures or fields (KTD9). Outputs left unchanged."

mkdir -p "$TMP/Headers/$FFI_MODULE" "$TMP/swift"
mv "$TMP/gen-mac/MeguriCoreBindings.swift" "$TMP/swift/"
mv "$TMP/gen-mac/$FFI_MODULE.h" "$TMP/gen-mac/module.modulemap" "$TMP/Headers/$FFI_MODULE/"
[ -z "$(ls -A "$TMP/gen-mac")" ] || die "unexpected bindgen output: $(ls "$TMP/gen-mac" | tr '\n' ' ')"

# --- XCFramework ------------------------------------------------------------
xcf_args=()
for s in $(printf '%s' "$slices" | tr ',' ' '); do
  xcf_args+=(-library "$(lib_of "$s")" -headers "$TMP/Headers")
done
xcodebuild -create-xcframework "${xcf_args[@]}" -output "$TMP/$FFI_MODULE.xcframework" >"$TMP/xcodebuild.log" 2>&1 8>&- 9>&- || {
  cat "$TMP/xcodebuild.log" >&2
  die "xcodebuild -create-xcframework failed"
}
# xcodebuild lists the libraries in a random order; sort them so an
# unchanged slice set gives a byte-identical Info.plist.
plist="$TMP/$FFI_MODULE.xcframework/Info.plist"
plutil -convert json -o "$TMP/plist.json" "$plist" &&
  jq -S '.AvailableLibraries |= sort_by(.LibraryIdentifier)' "$TMP/plist.json" >"$TMP/plist-sorted.json" &&
  plutil -convert xml1 -o "$plist" "$TMP/plist-sorted.json" ||
  die "could not normalize $plist"

# --- Compare before replace -------------------------------------------------
write_stamp() { # write_stamp <hash>
  printf 'hash=%s\nconfiguration=%s\nprofile=%s\nfeatures=%s\nslices=%s\n' \
    "$1" "$configuration" "$profile" "$features" "$slices" >"$STAMP.tmp" &&
    mv -f "$STAMP.tmp" "$STAMP"
}

# sync_tree <new dir> <output dir>: copy files whose bytes differ, remove
# files the new tree lacks. Unchanged files keep their mtimes. Any failure
# stops the script with the stamp still marked incomplete.
replaced=()
sync_tree() {
  local new="$1" out="$2" rel
  mkdir -p "$out"
  while IFS= read -r rel; do
    if ! cmp -s "$new/$rel" "$out/$rel"; then
      mkdir -p "$(dirname "$out/$rel")"
      { cp "$new/$rel" "$out/$rel.tmp.$$" && mv -f "$out/$rel.tmp.$$" "$out/$rel"; } ||
        die "could not replace $out/$rel; the next run rebuilds the outputs"
      replaced+=("${out#"$ROOT"/}/$rel")
    fi
  done < <(cd "$new" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
  while IFS= read -r rel; do
    [ -e "$new/$rel" ] || {
      rm -f "$out/$rel" || die "could not remove $out/$rel; the next run rebuilds the outputs"
      replaced+=("${out#"$ROOT"/}/$rel")
    }
  done < <(cd "$out" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
  find "$out" -mindepth 1 -type d -empty -delete
}

# No input hash matches `incomplete`: a replace that fails or is interrupted
# makes the next run rebuild, and the stamp still names the slices to keep.
write_stamp incomplete || die "could not write $STAMP"
sync_tree "$TMP/$FFI_MODULE.xcframework" "$XCF"
sync_tree "$TMP/swift" "$BINDINGS_DIR"
write_stamp "$hash" || die "could not write $STAMP"

echo "build-core-xcframework: built $configuration ($slices); ${#replaced[@]} output file(s) replaced" >&2
for f in ${replaced[@]+"${replaced[@]}"}; do echo "  replaced $f" >&2; done
write_report built ${replaced[@]+"${replaced[@]}"}
