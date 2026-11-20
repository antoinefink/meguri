# Controlled producer doubles for tooling tests only; no native/VM evidence.
# Sources the real bridge and xcode preparation functions.
unit_fixture_setup() {
  export MG_ROOT="$1" MG_JOB=xcode-check MG_RUN_ID="${2:-unit-input-run}"
  export MG_RUN_DIR="$MG_ROOT/build/mg/xcode-check/$MG_RUN_ID"
  export UNIT_CALLS="$MG_ROOT/build/unit-calls" UNIT_COMPILER="$MG_ROOT/build/native-target/debug/meguri-cli"
  mkdir -p "$MG_ROOT/tools/scripts" "$MG_ROOT/core" "$MG_RUN_DIR" "$(dirname "$UNIT_COMPILER")" "$MG_ROOT/build/unit-bin"
  local producer="$MG_ROOT/tools/mg"
  if [ "${3:-}" = --with-mg-dispatch ]; then
    # Preserve real MG routing/admission; intercept only the exact producer
    # command checked by the existing double below, in this generated repo.
    mv "$MG_ROOT/tools/mg" "$MG_ROOT/tools/mg-fixture-dispatch"
    producer="$MG_ROOT/build/unit-bin/core-test-double"
    cat >"$MG_ROOT/tools/mg" <<'DISPATCH'
#!/bin/bash
fixture_tools="$(cd "$(dirname "$0")" && pwd -P)"
if [ "${1:-}" = core-test ]; then
  exec "$fixture_tools/../build/unit-bin/core-test-double" "$@"
fi
exec "$fixture_tools/mg-fixture-dispatch" "$@"
DISPATCH
    chmod +x "$MG_ROOT/tools/mg"
    cat >"$MG_ROOT/tools/mg.d/zz-unit-producer-fakes.sh" <<'ENV'
# Controlled producer doubles only; the real input guard remains installed.
export UNIT_CALLS="$MG_ROOT/build/unit-calls"
export UNIT_COMPILER="$MG_ROOT/build/native-target/debug/meguri-cli"
export PATH="$MG_ROOT/build/unit-bin:$PATH"
ENV
  fi
  cp "$REPO_ROOT/tools/scripts/xcode-unit-inputs.py" \
    "$REPO_ROOT/tools/scripts/swift-cloud-service-helper.py" \
    "$REPO_ROOT/tools/scripts/native-media-review-capture.py" \
    "$REPO_ROOT/tools/scripts/native-media-export-capture.py" "$MG_ROOT/tools/scripts/"
  printf '#!/bin/sh\nexit 97\n' >"$UNIT_COMPILER"
  chmod 755 "$UNIT_COMPILER"
  cat >"$MG_ROOT/build/unit-bin/cargo" <<'CARGO'
#!/bin/bash
[ "$*" = 'build -q -p meguri-cli --message-format=json' ] || exit 97
[ "$PWD" = "$MG_ROOT/core" ] || exit 97
echo native-cargo >>"$UNIT_CALLS"
[ "${UNIT_CARGO_FAIL:-0}" = 0 ] || exit 1
python3 - <<'PY'
import json, os
if os.environ.get('UNIT_WARNING'):
    print(json.dumps({'reason': 'compiler-message', 'message': {'level': 'warning'}}))
print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': 'meguri-cli'},
                  'executable': os.environ['UNIT_COMPILER']}))
PY
CARGO
  cat >"$producer" <<'MG'
#!/bin/bash
[ "$*" = 'core-test -- --test ffi_it -E package(meguri-ffi) & test(=cloud_initial_app_fixture::export_initial_app_adoption_replies_from_real_publication)' ] || exit 97
echo rust-producer >>"$UNIT_CALLS"
[ "${UNIT_PRODUCER_FAIL:-0}" = 0 ] || exit 1
out="$MG_ROOT/${MEGURI_INITIAL_ADOPTION_FIXTURE#/work/}"
[ "$out" = "$MG_RUN_DIR/initial-adoption-fixture.json" ] || exit 97
[ "${UNIT_MISSING_FIXTURE:-0}" = 1 ] || printf '{"synthetic":"producer-double-only"}\n' >"$out"
result="$MG_ROOT/build/mg/core-test/$MG_RUN_ID-producer/result.json"
mkdir -p "$(dirname "$result")"
python3 - "$result" <<'PY'
import json, os, subprocess, sys
r = {'job': 'core-test', 'status': 'pass', 'dry_run': False, 'root': os.environ['MG_ROOT'],
     'run_id': os.environ['MG_RUN_ID'] + '-producer', 'args': sys.argv[2:],
     'commit': {'sha': subprocess.check_output(['git', '-C', os.environ['MG_ROOT'], 'rev-parse', 'HEAD'], text=True).strip(),
                'dirty': bool(os.environ.get('UNIT_DIRTY_RECEIPT'))},
     'summary': {'total': 1, 'passed': 1, 'failed': 0, 'skipped': 0}}
r['args'] = ['--', '--test', 'ffi_it', '-E',
             'package(meguri-ffi) & test(=cloud_initial_app_fixture::export_initial_app_adoption_replies_from_real_publication)']
if os.environ.get('UNIT_WRONG_REVISION'): r['commit']['sha'] = '0' * 40
if os.environ.get('UNIT_SKIPPED'): r['summary']['skipped'] = 1
with open(sys.argv[1], 'w') as f: json.dump(r, f)
PY
printf '%s\n' "$result"
MG
  chmod +x "$MG_ROOT/build/unit-bin/cargo" "$producer"
  export PATH="$MG_ROOT/build/unit-bin:$PATH"
  printf '/build/\n' >"$MG_ROOT/.gitignore"
  # Only the generated test repository is committed; never the real checkout.
  (unset GIT_DIR GIT_WORK_TREE; git -C "$MG_ROOT" init -q &&
    git -C "$MG_ROOT" add -A &&
    git -C "$MG_ROOT" -c user.name=Test -c user.email=test@example.invalid commit -qm 'Generated tooling fixture' --allow-empty) || return
}

unit_fixture_prepare() {
  (
    mg_job() { :; }
    mg_failure() { printf 'failure=%s\n' "$1"; }
    mg_status() { printf 'status=%s\n' "$1"; }
    mg_artifact() { :; }
    mg_detail() { :; }
    source "$REPO_ROOT/tools/mg.d/bridge.sh"
    source "$REPO_ROOT/tools/mg.d/xcode.sh"
    mg__xcode_unit_inputs
  )
}
