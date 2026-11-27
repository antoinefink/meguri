#!/usr/bin/env bats
# Real policy, synthetic Xcode/codesign/otool output; no Apple tool or account.
bats_require_minimum_version 1.5.0
REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"

setup() {
  export FX="$BATS_TEST_TMPDIR/facts"
  mkdir -p "$FX/bin"
  export PATH="$FX/bin:$PATH"
  export OTOOL_LOG="$FX/otool.log"
  cat >"$FX/bin/otool" <<'SH'
#!/bin/bash
[ "$1" = -L ] && [ -f "$2" ] || exit 97
printf '%s\n' "$2" >>"$OTOOL_LOG"
cat "$2"
SH
  cat >"$FX/bin/codesign" <<'SH'
#!/bin/bash
[ "$1 $2 $3 $4" = '-d --entitlements - --xml' ] || exit 97
cat "$5.grants"
SH
  cat >"$FX/bin/plutil" <<'PY'
#!/usr/bin/env python3
import json, plistlib, sys
raw = sys.stdin.buffer.read() if sys.argv[-1] == '-' else open(sys.argv[-1], 'rb').read()
print(json.dumps(plistlib.loads(raw)))
PY
  chmod +x "$FX/bin/"*
  python3 - "$FX" <<'PY'
import json, plistlib, sys
from pathlib import Path
root = Path(sys.argv[1])
settings = []
allowed = {'com.apple.security.app-sandbox': True,
           'com.apple.security.application-groups': ['TESTTEAM01.com.antoinefink.meguri.dev'],
           'keychain-access-groups': ['TESTTEAM01.com.antoinefink.meguri.dev']}
app = root / 'Meguri.app'
for target, bundle in [('Meguri', app), ('MeguriWidget', app / 'Contents/PlugIns/MeguriWidget.appex')]:
    binaries = bundle / 'Contents/MacOS'
    binaries.mkdir(parents=True)
    settings.append({'target': target, 'buildSettings': {'MEGURI_BUNDLE_ID_SUFFIX': '.dev',
        'PRODUCT_BUNDLE_IDENTIFIER': 'com.antoinefink.meguri.dev' + ('.widget' if target == 'MeguriWidget' else '')}})
    (bundle / 'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable': target}))
    for name in [target, target + '.debug.dylib']:
        (binaries / name).write_text('/System/Library/Frameworks/Foundation.framework/Foundation\n')
        Path(str(binaries / name) + '.grants').write_bytes(plistlib.dumps({}))
    Path(str(bundle) + '.grants').write_bytes(plistlib.dumps(allowed))
(root / 'settings.json').write_text(json.dumps(settings))
# Ordinary XCTest runners outside the app remain supported.
(root / 'MeguriUITests-Runner.app').mkdir()
PY
}

check_product() {
  run python3 -B "$REPO_ROOT/tools/tests/sync_build_configuration.py" \
    --settings "$FX/settings.json" --configuration Debug --platform mac --signing team --app "$FX/Meguri.app"
}

@test "allowed products inspect both executables and both optional debug dylibs" {
  check_product
  [ "$status" -eq 0 ]
  [ "$(wc -l <"$OTOOL_LOG" | tr -d ' ')" -eq 4 ]
  grep -F 'Meguri.debug.dylib' "$OTOOL_LOG"
  grep -F 'MeguriWidget.debug.dylib' "$OTOOL_LOG"
  rm "$FX/Meguri.app/Contents/MacOS/Meguri.debug.dylib" \
    "$FX/Meguri.app/Contents/PlugIns/MeguriWidget.appex/Contents/MacOS/MeguriWidget.debug.dylib"
  check_product
  [ "$status" -eq 0 ]
}

@test "each target rejects retired resolved settings including injected Info and linkage" {
  cp "$FX/settings.json" "$FX/good.json"
  for target in Meguri MeguriWidget; do
    for setting in MEGURI_CLOUD_CONTAINER INFOPLIST_KEY_MeguriCloudEnvironment OTHER_LDFLAGS INFOPLIST_KEY_UIBackgroundModes; do
      python3 - "$FX" "$target" "$setting" <<'PY'
import json, sys
from pathlib import Path
root, target, key = Path(sys.argv[1]), *sys.argv[2:]
data = json.loads((root / 'good.json').read_text())
next(t['buildSettings'] for t in data if t['target'] == target)[key] = 'remote-notification' if key.endswith('UIBackgroundModes') else '-framework CloudKit'
(root / 'settings.json').write_text(json.dumps(data))
PY
      check_product
      [ "$status" -eq 1 ]
    done
  done
}

@test "both Info plists and all signed executables reject retired grants and remote notifications" {
  for bundle in "$FX/Meguri.app" "$FX/Meguri.app/Contents/PlugIns/MeguriWidget.appex"; do
    for file in "$bundle/Contents/Info.plist" "$bundle.grants" "$bundle"/Contents/MacOS/*.dylib.grants; do
      cp "$file" "$FX/saved.plist"
      for key in MeguriCloudContainer com.apple.developer.icloud-services com.apple.developer.ubiquity-kvstore-identifier aps-environment com.apple.developer.aps-environment UIBackgroundModes; do
        python3 - "$file" "$key" <<'PY'
import plistlib, sys
from pathlib import Path
path, key = Path(sys.argv[1]), sys.argv[2]
data = plistlib.loads(path.read_bytes())
data[key] = ['remote-notification'] if key == 'UIBackgroundModes' else 'retired'
path.write_bytes(plistlib.dumps(data))
PY
        check_product
        [ "$status" -eq 1 ]
        cp "$FX/saved.plist" "$file"
      done
    done
  done
}

@test "CloudKit and stale app XCTest injection fail in main or debug app and widget implementations" {
  for binary in "$FX/Meguri.app"/Contents/MacOS/Meguri{,.debug.dylib} \
      "$FX/Meguri.app/Contents/PlugIns/MeguriWidget.appex"/Contents/MacOS/MeguriWidget{,.debug.dylib}; do
    cp "$binary" "$FX/saved.binary"
    for framework in CloudKit XCTest; do
      printf '/System/Library/Frameworks/%s.framework/%s\n' "$framework" "$framework" >"$binary"
      check_product
      [ "$status" -eq 1 ]
    done
    cp "$FX/saved.binary" "$binary"
  done
}

@test "both products reject the NSUbiquitousContainers family in Info and injected settings" {
  cp "$FX/settings.json" "$FX/good.json"
  local target bundle key
  for target in Meguri MeguriWidget; do
    bundle="$FX/Meguri.app"
    [ "$target" = Meguri ] || bundle="$bundle/Contents/PlugIns/MeguriWidget.appex"
    cp "$bundle/Contents/Info.plist" "$FX/good-info.plist"
    for key in NSUbiquitousContainers NSUbiquitousContainersSynthetic; do
      python3 - "$bundle/Contents/Info.plist" "$key" <<'PY'
import plistlib, sys
from pathlib import Path
path, key = Path(sys.argv[1]), sys.argv[2]
data = plistlib.loads(path.read_bytes())
data[key] = {'synthetic.container': {'NSUbiquitousContainerIsDocumentScopePublic': False}}
path.write_bytes(plistlib.dumps(data))
PY
      check_product
      [ "$status" -eq 1 ]
      cp "$FX/good-info.plist" "$bundle/Contents/Info.plist"
      python3 - "$FX" "$target" "$key" <<'PY'
import json, sys
from pathlib import Path
root, target, key = Path(sys.argv[1]), *sys.argv[2:]
data = json.loads((root / 'good.json').read_text())
next(t['buildSettings'] for t in data if t['target'] == target)['INFOPLIST_KEY_' + key] = {
    'synthetic.container': {'NSUbiquitousContainerIsDocumentScopePublic': False}}
(root / 'settings.json').write_text(json.dumps(data))
PY
      check_product
      [ "$status" -eq 1 ]
      cp "$FX/good.json" "$FX/settings.json"
    done
  done
  check_product
  [ "$status" -eq 0 ]
}

@test "allowed settings retain all configurations platforms and supported preview identities" {
  run python3 -B - "$REPO_ROOT/tools/tests" <<'PY'
import sys
sys.path.insert(0, sys.argv[1])
from sync_build_configuration import check
for configuration in ('Debug', 'Profile', 'Release', 'AppStore'):
    for platform in ('mac', 'ios'):
        for signing in ('adhoc', 'team'):
            for target in ('Meguri', 'MeguriWidget'):
                suffix = '.dev' if configuration in ('Debug', 'Profile') else ''
                settings = {'MEGURI_BUNDLE_ID_SUFFIX': suffix,
                            'PRODUCT_BUNDLE_IDENTIFIER': 'com.antoinefink.meguri' + suffix + ('.widget' if target == 'MeguriWidget' else '')}
                check(settings, configuration, platform, signing, target=target)
                if configuration == 'Release' and signing == 'team':
                    check(settings, configuration, platform, signing, target=target, real_copy=True)
                    settings['MEGURI_BUNDLE_ID_SUFFIX'] = '.dev'
                    settings['PRODUCT_BUNDLE_IDENTIFIER'] = 'com.antoinefink.meguri.dev' + ('.widget' if target == 'MeguriWidget' else '')
                    check(settings, configuration, platform, signing, target=target, personal_preview=True)
PY
  [ "$status" -eq 0 ]
}
