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
# A run hashes every non-ignored file under core/, this script, the
# configuration and the slice list. When the hash equals the stamp and the
# outputs exist, it exits without invoking cargo. Otherwise it builds each
# slice, checks that every slice exports the same uniffi_* symbols, generates
# the bindings from the Mac slice, creates the XCFramework in a temporary
# directory and replaces an output file only when its bytes differ, so Xcode
# and SwiftPM recompile and relink only what changed. Outputs and the stamp
# are untouched when any step fails.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
SELF="$ROOT/tools/scripts/build-core-xcframework.sh"
PKG="$ROOT/app/Packages/MeguriCore"
XCF="$PKG/Binaries/MeguriCoreFFI.xcframework"
STAMP="$PKG/Binaries/.stamp"
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

# Without an explicit --slices, keep the slices already built for this
# configuration, so a test run never drops the device slice and a
# pre-action only adds one.
if [ "$xcode" = true ] || [ -z "$slices_arg" ]; then
  [ -n "$slices_arg" ] || slices_arg=mac,sim
  if [ "$(stamp_value configuration)" = "$configuration" ]; then
    slices_arg="$slices_arg,$(stamp_value slices)"
  fi
fi
slices="$(printf '%s\n' mac $slices_arg | tr ',' '\n' | sed '/^$/d' | sort -u | tr '\n' ',' | sed 's/,$//')"
for s in $(printf '%s' "$slices" | tr ',' ' '); do slice_triple "$s" >/dev/null; done

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
# Every file under core/ that git tracks or would track (new files count
# before they are added), with its content id, plus this script and the
# build parameters. git hash-object hashes in one process; shasum is a perl
# script and too slow to start for the no-change budget.
inputs_manifest() {
  git -C "$ROOT" ls-files -co --exclude-standard --deduplicate -- core | LC_ALL=C sort >"$TMP/inputs"
  # Tracked files deleted from the work tree have no content to hash.
  git -C "$ROOT" ls-files -d -- core | LC_ALL=C sort >"$TMP/deleted"
  if [ -s "$TMP/deleted" ]; then
    LC_ALL=C comm -23 "$TMP/inputs" "$TMP/deleted" >"$TMP/kept" && mv "$TMP/kept" "$TMP/inputs"
  fi
  echo tools/scripts/build-core-xcframework.sh >>"$TMP/inputs"
  sed "s|^|$ROOT/|" "$TMP/inputs" | git -C "$ROOT" hash-object --stdin-paths >"$TMP/ids"
  printf 'configuration=%s\nprofile=%s\nfeatures=%s\nslices=%s\n' "$configuration" "$profile" "$features" "$slices"
  paste "$TMP/inputs" "$TMP/ids"
}

TMP="$(mktemp -d "${TMPDIR:-/tmp}/meguri-xcf.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
hash="$(inputs_manifest | git -C "$ROOT" hash-object --stdin)"

outputs_present() {
  [ -f "$XCF/Info.plist" ] && [ -f "$BINDINGS" ] || return 1
  local s
  for s in $(printf '%s' "$slices" | tr ',' ' '); do
    [ -f "$XCF/$(slice_dir "$s")/libmeguri_ffi.a" ] || return 1
  done
}

if [ "$(stamp_value hash)" = "$hash" ] && outputs_present; then
  echo "build-core-xcframework: up to date ($configuration; $slices)" >&2
  write_report up-to-date
  exit 0
fi

# --- Build each slice -------------------------------------------------------
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || die "cargo not found; install rustup (tools/mg doctor)"

# Xcode puts its build settings in a pre-action's environment. An iOS
# SDKROOT or deployment target would leak into the host-side build scripts
# and proc macros, so cargo runs without them and with explicit targets.
cargo_env() {
  env -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET -u IPHONEOS_DEPLOYMENT_TARGET \
    -u TVOS_DEPLOYMENT_TARGET -u WATCHOS_DEPLOYMENT_TARGET -u XROS_DEPLOYMENT_TARGET \
    -u PLATFORM_NAME -u ARCHS -u CONFIGURATION "$@"
}

feature_args=(--no-default-features)
[ -z "$features" ] || feature_args+=(--features "$features")
target_root="${CARGO_TARGET_DIR:-$ROOT/core/target}"
case "$target_root" in /*) ;; *) target_root="$ROOT/core/$target_root" ;; esac

for s in $(printf '%s' "$slices" | tr ',' ' '); do
  triple="$(slice_triple "$s")"
  echo "build-core-xcframework: cargo build $triple ($configuration: $profile${features:+, $features})" >&2
  (cd "$ROOT/core" && cargo_env MACOSX_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET" \
    IPHONEOS_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET" \
    cargo build --locked -p meguri-ffi --lib --target "$triple" --profile "$profile" "${feature_args[@]}") ||
    die "cargo build failed for $triple"
done

lib_of() { echo "$target_root/$(slice_triple "$1")/$profile_dir/libmeguri_ffi.a"; }

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
[ -s "$TMP/symbols-mac" ] || die "no uniffi_* symbols in $(lib_of mac)"
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
mkdir -p "$TMP/gen" "$TMP/Headers/$FFI_MODULE" "$TMP/swift"
(cd "$ROOT/core" && "$bindgen" "$(lib_of mac)" "$TMP/gen" --swift-sources --headers --modulemap \
  --module-name "$FFI_MODULE" --modulemap-filename module.modulemap --metadata-no-deps) ||
  die "uniffi-bindgen-swift failed"
mv "$TMP/gen/MeguriCoreBindings.swift" "$TMP/swift/"
mv "$TMP/gen/$FFI_MODULE.h" "$TMP/gen/module.modulemap" "$TMP/Headers/$FFI_MODULE/"
[ -z "$(ls -A "$TMP/gen")" ] || die "unexpected bindgen output: $(ls "$TMP/gen" | tr '\n' ' ')"

# --- XCFramework ------------------------------------------------------------
xcf_args=()
for s in $(printf '%s' "$slices" | tr ',' ' '); do
  xcf_args+=(-library "$(lib_of "$s")" -headers "$TMP/Headers")
done
xcodebuild -create-xcframework "${xcf_args[@]}" -output "$TMP/$FFI_MODULE.xcframework" >"$TMP/xcodebuild.log" 2>&1 || {
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
# sync_tree <new dir> <output dir>: copy files whose bytes differ, remove
# files the new tree lacks. Unchanged files keep their mtimes.
replaced=()
sync_tree() {
  local new="$1" out="$2" rel
  mkdir -p "$out"
  while IFS= read -r rel; do
    if ! cmp -s "$new/$rel" "$out/$rel"; then
      mkdir -p "$(dirname "$out/$rel")"
      cp "$new/$rel" "$out/$rel.tmp.$$" && mv -f "$out/$rel.tmp.$$" "$out/$rel"
      replaced+=("${out#"$ROOT"/}/$rel")
    fi
  done < <(cd "$new" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
  while IFS= read -r rel; do
    [ -e "$new/$rel" ] || {
      rm -f "$out/$rel"
      replaced+=("${out#"$ROOT"/}/$rel")
    }
  done < <(cd "$out" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
  find "$out" -mindepth 1 -type d -empty -delete
}
sync_tree "$TMP/$FFI_MODULE.xcframework" "$XCF"
sync_tree "$TMP/swift" "$BINDINGS_DIR"

printf 'hash=%s\nconfiguration=%s\nprofile=%s\nfeatures=%s\nslices=%s\n' \
  "$hash" "$configuration" "$profile" "$features" "$slices" >"$STAMP.tmp"
mv -f "$STAMP.tmp" "$STAMP"

echo "build-core-xcframework: built $configuration ($slices); ${#replaced[@]} output file(s) replaced" >&2
for f in ${replaced[@]+"${replaced[@]}"}; do echo "  replaced $f" >&2; done
write_report built ${replaced[@]+"${replaced[@]}"}
