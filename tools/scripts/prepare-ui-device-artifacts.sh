#!/bin/bash
# Physical UI runners cannot read BUILT_PRODUCTS_DIR on the build Mac.
# Keep only the app metadata and code that BuildUnderTest/PerfPreconditions read.
set -euo pipefail

[ "${PLATFORM_NAME:-}" = iphoneos ] || exit 0

fail() { echo "error: UI device artifacts: $*" >&2; exit 1; }
case "${CONFIGURATION:-}" in
  Debug|Profile) ;;
  *) fail "only Debug and Profile development builds are supported" ;;
esac

# Refuse traversal and symlink components before reading or removing anything.
plain_path() {
  local path="$1"
  case "$path/" in
    /*) ;;
    *) fail "expected an absolute build path" ;;
  esac
  case "$path/" in
    *//*|*/./*|*/../*) fail "non-canonical build path: $path" ;;
  esac
  while [ "$path" != / ]; do
    [ ! -L "$path" ] || fail "symlink build path: $path"
    path="${path%/*}"
    [ -n "$path" ] || path=/
  done
}

app="${BUILT_PRODUCTS_DIR:?}/Meguri.app"
# Xcode allows writes throughout this target-owned temporary directory. Its
# Resources phase copies the prepared folder into the test bundle before signing.
staging="${TARGET_TEMP_DIR:?}/MeguriBuildUnderTest"
plain_path "$app"
plain_path "$staging"
case "$staging/" in
  "$app/"*) fail "staging path overlaps the built app" ;;
esac
case "$app/" in
  "$staging/"*) fail "built app overlaps the staging path" ;;
esac
[ -d "$app" ] || fail "missing built app: $app"

info="$app/Info.plist"
[ -f "$info" ] && [ -r "$info" ] && [ ! -L "$info" ] || fail "missing or unreadable app Info.plist"
plutil -lint "$info" >/dev/null || fail "malformed app Info.plist"
# -n plus a sentinel preserves trailing newlines in malformed identifiers;
# command substitution must not silently turn them into valid names.
identifier="$(plutil -extract CFBundleIdentifier raw -expect string -n -o - "$info" && printf /)" || fail "missing app bundle identifier"
identifier="${identifier%/}"
[ "$identifier" = com.antoinefink.meguri.dev ] || fail "expected the development app bundle identifier"
executable="$(plutil -extract CFBundleExecutable raw -expect string -n -o - "$info" && printf /)" || fail "missing app executable identifier"
executable="${executable%/}"
case "$executable" in
  ''|.*|Info.plist|*[!A-Za-z0-9_.-]*) fail "invalid app executable identifier" ;;
esac

shopt -s nullglob dotglob
for binary in "$app/$executable" "$app/"*.dylib; do
  [ -f "$binary" ] && [ -r "$binary" ] && [ ! -L "$binary" ] || fail "missing, unreadable or symlink app binary: $binary"
done

# The fixed leaf belongs only to this phase. rm does not follow links within it;
# plain_path above also refuses a symlink at the leaf or any ancestor.
rm -rf "$staging"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging"
cp "$info" "$staging/Info.plist"
for binary in "$app/$executable" "$app/"*.dylib; do
  cp "$binary" "$staging/"
done
trap - EXIT
