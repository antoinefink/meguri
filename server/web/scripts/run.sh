#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
set -euo pipefail
node_version="$(node --version)"
npm_version="$(npm --version)"
printf '%s\n' "$node_version" "$npm_version"
[ "$npm_version" = 11.20.0 ]
tool_scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
rustc --version
node -e 'if (process.version !== "v26.13.1" || process.release.lts !== "Lithium") process.exit(1)'
mode="${1:?mode required}"
export npm_config_cache=/tmp/npm-cache
export PLAYWRIGHT_BROWSERS_PATH=/browsers
export XDG_CACHE_HOME=/tmp/xdg-cache
mkdir -p "$XDG_CACHE_HOME"
if [ "$mode" = probe ]; then
  {
    if command -v fc-list >/dev/null 2>&1; then
      printf '%s\n' 'Available font families (up to 40):'
      fc-list --format '%{family}\n' | LC_ALL=C sort -u | sed -n '1,40p'
    else
      printf '%s\n' 'fc-list unavailable in the pinned image.'
    fi
    if command -v fc-match >/dev/null 2>&1; then
      for family in system-ui ui-sans-serif sans-serif; do
        printf '%s: ' "$family"
        fc-match "$family"
      done
    else
      printf '%s\n' 'fc-match unavailable in the pinned image.'
    fi
  } | tee /out/font-resolution.txt
  exit 0
fi
if [ "$mode" = preview ]; then exec node /opt/sync-web/fixture.mjs; fi
if [ "$mode" = package-preview ]; then exec node /opt/sync-web/fixture.mjs package; fi
if [ "$mode" = actualserver ]; then
  mkdir -p /work/server /work/core
  cp -R /server/. /work/server/
  cp -R /core/. /work/core/
  cd /work/server
  export MEGURI_WEB_PREPARED_DIR=/prepared
  CARGO_NET_RETRY=0 cargo build --locked --bin meguri-sync-server 2>&1 | tee /out/cargo-build.log
  cp /cargo-target/debug/meguri-sync-server /out/meguri-sync-server
  sha256sum /out/meguri-sync-server Cargo.lock /prepared/meguri_web_assets.rs /prepared/embedded/* > /out/build-sha256.txt
  cp /prepared/assets.json /out/assets.json
  find /work/server /work/core -type f -print0 | sort -z | xargs -0 sha256sum > /out/server-source-sha256.txt
  exit 0
fi
# Shared locked preparation for normal server jobs and frozen candidates.
web_source="${MEGURI_WEB_SOURCE:-/source}"
web_work="${MEGURI_WEB_WORK:-/work}"
web_output="${MEGURI_WEB_OUTPUT:-/out}"
web_deps="${MEGURI_WEB_DEPS:-/deps}"
mkdir -p "$web_work" "$web_output"
case "$mode" in
  resolve|install|audit|browser-install)
    python3 "$tool_scripts/audit-applicability.py" check-inputs "$web_source" /audit-controls
    ;;
esac
# Preserve raw npm bytes, diagnostic bytes, and its actual exit independently.
# Only the fixed helper decides whether this exact build assessment applies.
audit_frontend() {
  local raw_exit=0
  npm audit --fetch-retries=0 --json > "$web_output/npm-audit.json" 2> "$web_output/npm-audit-stderr.txt" || raw_exit=$?
  printf '%s\n' "$raw_exit" > "$web_output/npm-audit-exit.txt"
  printf '{"node":"%s","npm":"%s"}\n' "$node_version" "$npm_version" > "$web_output/npm-audit-tools.json"
  python3 "$tool_scripts/audit-applicability.py" assess "$web_source" "$web_work" /audit-controls "$web_output"
}
node "${web_source}/scripts/copy-source.mjs" "$web_source" "$web_work"
cd "${web_work}"
case "$mode" in
  resolve|install)
    mkdir -p "${web_deps}/cache"
    export npm_config_cache="${web_deps}/cache"
    if [ "$mode" = resolve ]; then
      npm install --package-lock-only --ignore-scripts --save-exact --no-audit --no-fund --engine-strict --fetch-retries=0
      cp package-lock.json "${web_output}/package-lock.json"
    fi
    npm ci --ignore-scripts --no-audit --no-fund --engine-strict --fetch-retries=0
    npm ls --all --json > "${web_output}/dependency-graph.json"
    node scripts/notices.mjs "${web_work}" "${web_output}"
    audit_frontend
    # Metadata and publication times are retained for every locked package name.
    node scripts/metadata.mjs "${web_work}/package-lock.json" "${web_output}/npm-metadata.json"
    cp package.json package-lock.json "${web_deps}/"
    cp "${web_output}/runtime-notices.txt" "${web_deps}/runtime-notices.txt"
    ;;
  *)
    cmp package-lock.json "${web_deps}/package-lock.json"
    cmp package.json "${web_deps}/package.json"
    cp -R "${web_deps}/cache" /tmp/npm-cache
    npm ci --offline --ignore-scripts --no-audit --no-fund --engine-strict --fetch-retries=0
    export MEGURI_WEB_NOTICES_FILE="${web_deps}/runtime-notices.txt"
    case "$mode" in
      typecheck) npm run typecheck ;;
      unit)
        npm exec --offline -- tsc -p tsconfig.tests.json
        node --test --test-reporter=tap --test-reporter-destination="${web_output}/unit-results.tap" .test-build/tests/*.test.js
        ;;
      build|check)
        if [ "$mode" = check ]; then
          npm run typecheck
          npm exec --offline -- tsc -p tsconfig.tests.json
          node --test --test-reporter=tap --test-reporter-destination="${web_output}/unit-results.tap" .test-build/tests/*.test.js
        fi
        node scripts/notices.mjs "${web_work}" "${web_output}"
        export MEGURI_WEB_NOTICES_FILE="${web_output}/runtime-notices.txt"
        npm run build
        node scripts/embed.mjs dist "${web_output}/prepared"
        cp -R dist "${web_output}/dist"
        find "${web_source}" -type f -print0 | sort -z | xargs -0 sha256sum > "${web_output}/source-sha256.txt"
        ;;
      audit)
        npm ls --all --json > "${web_output}/dependency-graph.json"
        node scripts/notices.mjs "${web_work}" "${web_output}"
        audit_frontend
        node scripts/metadata.mjs "${web_work}/package-lock.json" "${web_output}/npm-metadata.json"
        ;;
      browser-install)
        # Installation is explicit, after a successful locked graph audit.
        audit_frontend
        node scripts/notices.mjs "${web_work}" "${web_output}"
        cp node_modules/playwright-core/browsers.json "${web_output}/browsers.json"
        node node_modules/playwright/cli.js install chromium
        ;;
      package-browser)
        # Use this image's prepared locked graph and Chromium, never the runtime.
        cp /package-smoke.mjs ./package-smoke.mjs
        cp node_modules/playwright-core/browsers.json "${web_output}/package-browsers-${2:?phase required}.json"
        node ./package-smoke.mjs "${2:?phase required}" browser
        ;;
      browser)
        test -f /fixture/fixture.json
        mkdir -p "${web_output}/browser-artifacts"
        cp node_modules/playwright-core/browsers.json "${web_output}/browsers.json"
        npm run e2e
        ;;
      *) echo 'Unknown sync-web mode' >&2; exit 2 ;;
    esac
    ;;
esac
