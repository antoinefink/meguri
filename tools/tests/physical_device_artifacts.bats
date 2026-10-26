#!/usr/bin/env bats
# Exercise staging with synthetic app bytes and a plist-aware plutil fake.
# No Xcode, signing, installation or device access.
bats_require_minimum_version 1.5.0

REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
SCRIPT="$REPO_ROOT/tools/scripts/prepare-ui-device-artifacts.sh"

setup() {
  local tmp
  tmp="$(cd "$BATS_TEST_TMPDIR" && pwd -P)"
  export PLATFORM_NAME=iphoneos CONFIGURATION=Profile
  export BUILT_PRODUCTS_DIR="$tmp/products with spaces" TARGET_BUILD_DIR="$tmp/test products"
  export TARGET_TEMP_DIR="$tmp/UI-test target temp"
  export UNLOCALIZED_RESOURCES_FOLDER_PATH=MeguriUITests.xctest
  APP="$BUILT_PRODUCTS_DIR/Meguri.app"
  RESOURCES="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
  STAGE="$TARGET_TEMP_DIR/MeguriBuildUnderTest"
  BIN="$tmp/bin"
  mkdir -p "$APP" "$RESOURCES" "$TARGET_TEMP_DIR" "$BIN"
  export PLUTIL_LOG="$tmp/plutil.log"
  : >"$PLUTIL_LOG"
  cat >"$BIN/plutil" <<'PY'
#!/usr/bin/env python3
import plistlib
import os
import sys

args = sys.argv[1:]
with open(os.environ["PLUTIL_LOG"], "a") as log:
    log.write(repr(args) + "\n")
try:
    with open(args[-1], "rb") as source:
        info = plistlib.load(source)
    if args[0] == "-lint":
        assert len(args) == 2
    else:
        assert args[0] == "-extract" and args[2:-1] == ["raw", "-expect", "string", "-n", "-o", "-"]
        value = info[args[1]]
        assert isinstance(value, str)
        sys.stdout.write(value)
except Exception:
    sys.exit(1)
PY
  chmod +x "$BIN/plutil"
  export PATH="$BIN:$PATH"
  write_info
  # A custom CFBundleExecutable proves that the name is read from the plist.
  printf '\317\372\355\376\000__llvm_prf_cnts\000app-code\377' >"$APP/AppExecutable"
  printf '\317\372\355\376\000debug-code\377' >"$APP/Meguri.debug.dylib"
}

write_info() {
  python3 - "$APP/Info.plist" "${1-com.antoinefink.meguri.dev}" "${2-AppExecutable}" "${3-xml}" <<'PY'
import plistlib
import sys

path, identifier, executable, fmt = sys.argv[1:]
with open(path, "wb") as out:
    plistlib.dump({"CFBundleIdentifier": identifier, "CFBundleExecutable": executable,
                  "MeguriAppGroupIdentifier": "group." + identifier}, out,
                 fmt=plistlib.FMT_BINARY if fmt == "binary" else plistlib.FMT_XML)
PY
}

stage() { run /bin/bash "$SCRIPT"; }

@test "physical device artifacts: copy actual app bytes in Debug and Profile" {
  local configuration
  printf '\000preview-code\377' >"$APP/__preview.dylib"
  mkdir -p "$APP/Fixtures" "$APP/Frameworks" "$APP/PlugIns"
  printf 'excluded' >"$APP/Fixtures/media.mp4"
  printf 'excluded' >"$APP/embedded.mobileprovision"
  printf 'excluded' >"$APP/certificate.cer"
  for configuration in Debug Profile; do
    export CONFIGURATION="$configuration"
    write_info com.antoinefink.meguri.dev AppExecutable binary
    stage
    [ "$status" -eq 0 ]
    cmp "$APP/Info.plist" "$STAGE/Info.plist"
    cmp "$APP/AppExecutable" "$STAGE/AppExecutable"
    cmp "$APP/Meguri.debug.dylib" "$STAGE/Meguri.debug.dylib"
    cmp "$APP/__preview.dylib" "$STAGE/__preview.dylib"
    [ "$(find "$STAGE" -type f | wc -l)" -eq 4 ]
    [ "$(find "$STAGE" -mindepth 1 -type d | wc -l)" -eq 0 ]
    # Existing PerfPreconditions must still see an instrumentation marker.
    LC_ALL=C grep -a -q '__llvm_prf_cnts' "$STAGE/AppExecutable"
  done
}

@test "physical device artifacts: every rebuild clears only its fixed leaf" {
  stage
  [ "$status" -eq 0 ]
  mkdir "$STAGE/stale-directory"
  printf 'keep' >"$RESOURCES/other-resource"
  printf 'keep temp' >"$TARGET_TEMP_DIR/other-output"
  ln -s "$RESOURCES/other-resource" "$STAGE/stale-link"
  rm "$APP/Meguri.debug.dylib"
  printf 'new app bytes' >"$APP/AppExecutable"
  stage
  [ "$status" -eq 0 ]
  cmp "$APP/AppExecutable" "$STAGE/AppExecutable"
  [ ! -e "$STAGE/Meguri.debug.dylib" ]
  [ ! -e "$STAGE/stale-directory" ]
  [ ! -L "$STAGE/stale-link" ]
  [ "$(cat "$RESOURCES/other-resource")" = keep ]
  [ "$(cat "$TARGET_TEMP_DIR/other-output")" = 'keep temp' ]
  [ ! -e "$RESOURCES/MeguriBuildUnderTest" ]
}

@test "physical device artifacts: staging needs no test bundle paths" {
  rm -rf "$TARGET_BUILD_DIR"
  unset TARGET_BUILD_DIR UNLOCALIZED_RESOURCES_FOLDER_PATH
  stage
  [ "$status" -eq 0 ]
  cmp "$APP/Info.plist" "$STAGE/Info.plist"
  cmp "$APP/AppExecutable" "$STAGE/AppExecutable"
  [ ! -e "$RESOURCES" ]
}

@test "physical device artifacts: missing or malformed Info and absent executable fail" {
  rm "$APP/Info.plist"
  stage
  [ "$status" -ne 0 ]
  printf 'not a plist' >"$APP/Info.plist"
  stage
  [ "$status" -ne 0 ]
  write_info
  rm "$APP/AppExecutable"
  stage
  [ "$status" -ne 0 ]
  [ ! -e "$STAGE" ]
}

@test "physical device artifacts: missing or incorrectly typed metadata fails" {
  local field value
  for field in CFBundleIdentifier CFBundleExecutable; do
    for value in missing number; do
      write_info
      python3 - "$APP/Info.plist" "$field" "$value" <<'PY'
import plistlib
import sys

path, field, value = sys.argv[1:]
with open(path, "rb") as source:
    info = plistlib.load(source)
if value == "missing":
    del info[field]
else:
    info[field] = 17
with open(path, "wb") as out:
    plistlib.dump(info, out)
PY
      stage
      [ "$status" -ne 0 ]
      [ ! -e "$STAGE" ]
    done
  done
}

@test "physical device artifacts: production or unrelated ids and unsupported configurations fail" {
  local value
  for value in com.antoinefink.meguri com.example.other '' $'com.antoinefink.meguri.dev\n'; do
    write_info "$value"
    stage
    [ "$status" -ne 0 ]
  done
  write_info
  for value in Release AppStore Unknown ''; do
    export CONFIGURATION="$value"
    stage
    [ "$status" -ne 0 ]
  done
  [ ! -e "$STAGE" ]
}

@test "physical device artifacts: executable identifiers cannot escape or overwrite metadata" {
  local value
  for value in ../outside /outside nested/executable . .. '' Info.plist $'AppExecutable\n'; do
    write_info com.antoinefink.meguri.dev "$value"
    stage
    [ "$status" -ne 0 ]
    [ ! -e "$STAGE" ]
  done
}

@test "physical device artifacts: symlink metadata, executable, dylib and source directory fail" {
  local name
  for name in Info.plist AppExecutable Meguri.debug.dylib; do
    mv "$APP/$name" "$BUILT_PRODUCTS_DIR/saved-file"
    ln -s "$BUILT_PRODUCTS_DIR/saved-file" "$APP/$name"
    stage
    [ "$status" -ne 0 ]
    [ ! -e "$STAGE" ]
    rm "$APP/$name"
    mv "$BUILT_PRODUCTS_DIR/saved-file" "$APP/$name"
  done
  mv "$APP" "$BUILT_PRODUCTS_DIR/saved-app"
  ln -s "$BUILT_PRODUCTS_DIR/saved-app" "$APP"
  stage
  [ "$status" -ne 0 ]
  [ ! -e "$STAGE" ]
}

@test "physical device artifacts: symlink build roots or staging paths never redirect reads or deletion" {
  local products="$BUILT_PRODUCTS_DIR" target="$TARGET_TEMP_DIR"
  ln -s "$products" "$target/source-link"
  export BUILT_PRODUCTS_DIR="$target/source-link"
  stage
  [ "$status" -ne 0 ]
  export BUILT_PRODUCTS_DIR="$products"
  mkdir "$target/keep"
  printf keep >"$target/keep/sentinel"
  ln -s "$target/keep" "$STAGE"
  stage
  [ "$status" -ne 0 ]
  [ "$(cat "$target/keep/sentinel")" = keep ]
  [ -L "$STAGE" ]
  rm "$STAGE"
  mv "$target" "$target-saved"
  ln -s "$target-saved" "$target"
  stage
  [ "$status" -ne 0 ]
  [ -L "$target" ]
  [ "$(cat "$target-saved/keep/sentinel")" = keep ]
}

@test "physical device artifacts: traversal in Xcode directory inputs fails before staging" {
  local products="$BUILT_PRODUCTS_DIR" target="$TARGET_TEMP_DIR" value
  export BUILT_PRODUCTS_DIR="$products/../products with spaces"
  stage
  [ "$status" -ne 0 ]
  export BUILT_PRODUCTS_DIR="$products"
  for value in '' relative "$target/../UI-test target temp" "$target/." "$target//nested" "$target/"; do
    export TARGET_TEMP_DIR="$value"
    stage
    [ "$status" -ne 0 ]
    [ ! -e "$STAGE" ]
  done
  unset TARGET_TEMP_DIR
  stage
  [ "$status" -ne 0 ]
  [ ! -e "$STAGE" ]
}

@test "physical device artifacts: overlapping app and staging paths fail before changing either" {
  local target="$TARGET_TEMP_DIR"
  mkdir "$APP/MeguriBuildUnderTest"
  printf keep >"$APP/MeguriBuildUnderTest/sentinel"
  export TARGET_TEMP_DIR="$APP"
  stage
  [ "$status" -ne 0 ]
  [[ "$output" == *'staging path overlaps the built app'* ]]
  [ "$(cat "$APP/MeguriBuildUnderTest/sentinel")" = keep ]
  [ -f "$APP/Info.plist" ]
  [ -f "$APP/AppExecutable" ]

  export TARGET_TEMP_DIR="$target"
  mkdir "$STAGE"
  printf keep >"$STAGE/sentinel"
  mv "$APP" "$STAGE/Meguri.app"
  export BUILT_PRODUCTS_DIR="$STAGE"
  stage
  [ "$status" -ne 0 ]
  [[ "$output" == *'built app overlaps the staging path'* ]]
  [ "$(cat "$STAGE/sentinel")" = keep ]
  [ -f "$STAGE/Meguri.app/Info.plist" ]
  [ -f "$STAGE/Meguri.app/AppExecutable" ]
}

@test "physical device artifacts: copying errors fail the build" {
  printf '#!/bin/bash\nexit 42\n' >"$BIN/cp"
  chmod +x "$BIN/cp"
  stage
  [ "$status" -eq 42 ]
  [ ! -e "$STAGE" ]
}

@test "physical device artifacts: Mac and simulator do nothing even with unsupported config and absent inputs" {
  local platform
  mkdir "$STAGE"
  printf untouched >"$STAGE/sentinel"
  unset BUILT_PRODUCTS_DIR TARGET_TEMP_DIR TARGET_BUILD_DIR UNLOCALIZED_RESOURCES_FOLDER_PATH
  export CONFIGURATION=Release
  for platform in macosx iphonesimulator; do
    export PLATFORM_NAME="$platform"
    stage
    [ "$status" -eq 0 ]
    [ "$(cat "$STAGE/sentinel")" = untouched ]
    [ ! -s "$PLUTIL_LOG" ]
  done
}

@test "physical device artifacts: UI target dependency, phase ordering and sandbox declarations stay wired" {
  python3 - "$REPO_ROOT" <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path(sys.argv[1])
project = (root / "app/Meguri.xcodeproj/project.pbxproj").read_text()

def block(identifier):
    return re.search(r"\t\t" + identifier + r" /\*[^\n]*\*/ = \{\n(.*?)\n\t\t\};", project, re.S)[1]

target = block("4D4547555249000000000300")
phases = re.search(r"buildPhases = \((.*?)\);", target, re.S)[1]
assert phases.index("0302 /* Sources */") < phases.index("0303 /* Frameworks */")
assert phases.index("0303 /* Frameworks */") < phases.index("0305 /* Prepare UI device artifacts */")
assert phases.index("0305 /* Prepare UI device artifacts */") < phases.index("0304 /* Resources */")
assert "0320 /* PBXTargetDependency */" in re.search(r"dependencies = \((.*?)\);", target, re.S)[1]
assert "target = 4D4547555249000000000100 /* Meguri */;" in block("4D4547555249000000000320")
phase = block("4D4547555249000000000305")
assert "isa = PBXShellScriptBuildPhase;" in phase and "alwaysOutOfDate = 1;" in phase
inputs = re.search(r"inputPaths = \((.*?)\);", phase, re.S)[1]
assert re.findall(r'"([^"\n]+)"', inputs) == [
    "$(SRCROOT)/../tools/scripts/prepare-ui-device-artifacts.sh", "$(BUILT_PRODUCTS_DIR)/Meguri.app"]
outputs = re.search(r"outputPaths = \((.*?)\);", phase, re.S)[1]
assert re.findall(r'"([^"\n]+)"', outputs) == [
    "$(TARGET_TEMP_DIR)/MeguriBuildUnderTest"]
assert 'shellPath = /bin/bash;' in phase
def shell_script(phase):
    quoted = r'"(?:\\.|[^"\\])*"'
    array = r'\(\s*(?:' + quoted + r'\s*(?:,\s*' + quoted + r'\s*)*,?\s*)?\)'
    matches = re.findall(r'shellScript = (' + quoted + '|' + array + r');', phase, re.S)
    assert len(matches) == 1, "Expected one literal script field"
    raw = matches[0]
    if raw.startswith('('):
        return '\n'.join(json.loads(item) for item in re.findall(quoted, raw))
    return json.loads(raw)

expected_script = '/bin/bash "${SRCROOT}/../tools/scripts/prepare-ui-device-artifacts.sh"\n'
assert shell_script(phase) == expected_script
assert shell_script('shellScript = ' + json.dumps(expected_script) + ';') == expected_script
array_script = '(' + json.dumps(expected_script.rstrip('\n')) + ', "",)'
assert shell_script('shellScript = ' + array_script + ';') == expected_script
for unsupported in ['shellScript = 7;', 'shellScript = ("ok", unquoted,);']:
    try:
        shell_script(unsupported)
    except (AssertionError, ValueError):
        pass
    else:
        raise AssertionError("Unsupported script form accepted")
resources = block("4D4547555249000000000304")
assert "isa = PBXResourcesBuildPhase;" in resources
assert "0311 /* MeguriBuildUnderTest in Resources */" in re.search(r"files = \((.*?)\);", resources, re.S)[1]
build_file = re.search(r"4D4547555249000000000311 /\*[^\n]*\*/ = \{([^\n]+)\};", project)[1]
assert "isa = PBXBuildFile;" in build_file
assert "fileRef = 4D4547555249000000000024 /* MeguriBuildUnderTest */;" in build_file
file_ref = re.search(r"4D4547555249000000000024 /\*[^\n]*\*/ = \{([^\n]+)\};", project)[1]
assert "isa = PBXFileReference;" in file_ref
assert "lastKnownFileType = folder;" in file_ref
assert "path = MeguriBuildUnderTest;" in file_ref
assert "sourceTree = TARGET_TEMP_DIR;" in file_ref
root_group = re.search(r"\t\t4D4547555249000000000002 = \{\n(.*?)\n\t\t\};", project, re.S)[1]
assert "0024 /* MeguriBuildUnderTest */" in re.search(r"children = \((.*?)\);", root_group, re.S)[1]
ui_config = root / "app/Config/MeguriUITests.xcconfig"
assert re.search(r"^USE_RECURSIVE_SCRIPT_INPUTS_IN_SCRIPT_PHASES = YES$", ui_config.read_text(), re.M)
exclusions = re.findall(r"^EXCLUDED_SOURCE_FILE_NAMES(.*?) = (.*)$", ui_config.read_text(), re.M)
assert exclusions == [
    ("[sdk=macosx*]", "$(inherited) MeguriBuildUnderTest"),
    ("[sdk=iphonesimulator*]", "$(inherited) MeguriBuildUnderTest"),
]
for config in (root / "app/Config").glob("*.xcconfig"):
    if config != ui_config:
        assert "USE_RECURSIVE_SCRIPT_INPUTS_IN_SCRIPT_PHASES" not in config.read_text()
assert "USE_RECURSIVE_SCRIPT_INPUTS_IN_SCRIPT_PHASES" not in project
assert "ENABLE_USER_SCRIPT_SANDBOXING = YES" in (root / "app/Config/Base.xcconfig").read_text()
assert "ENABLE_USER_SCRIPT_SANDBOXING" not in project
assert "ENABLE_USER_SCRIPT_SANDBOXING" not in ui_config.read_text()
PY
}
