# Public compiler checks from neutral facts; no source checkout is mounted.
mg_job css-reference host mg_css_reference \
  "Verify card-runtime color text with the pinned public compiler in Docker: card-runtime"

mg_css_reference() {
  if [ "$#" -ne 1 ] || [ "${1:-}" != card-runtime ]; then
    mg_failure usage "usage: tools/mg css-reference card-runtime"
    mg_status error; return 0
  fi
  local role source runner out rc
  role="$(mg_session_role)"
  if mg_word_in "$role" "implementer fixer"; then
    mg_failure role "this spec-side compiler check refuses implementer and fixer sessions"
    mg_status error; return 0
  fi
  source="$MG_ROOT/specs/render/card-runtime-color-inputs.json"
  runner="$MG_TOOLS/scripts/research/card_runtime_colors.py"
  out="$MG_RUN_DIR/colors.json"
  if [ ! -f "$source" ] || [ -L "$source" ] || [ ! -f "$runner" ] || [ -L "$runner" ]; then
    mg_failure inputs "fixed compiler facts and runner must be regular files"
    mg_status error; return 0
  fi
  if ! mg_compose --progress quiet build image-tools >&2; then
    mg_failure image-build "the public compiler check's image could not be built"
    mg_status error; return 0
  fi
  docker run --rm --init --read-only --cap-drop ALL \
    --label "meguri.mg.run-id=$MG_RUN_ID" --tmpfs /tmp:exec,mode=1777 \
    -v "$source:/facts.json:ro" -v "$runner:/runner.py:ro" -v "$MG_RUN_DIR:/out" \
    "$(mg_compose_project)-image-tools" python3 -I -B /runner.py /facts.json /out/colors.json
  rc=$?
  if [ "$rc" -gt 1 ] || [ ! -f "$out" ] || ! jq -e '
    .schema_version==1 and .compiler.version=="1.102.0" and
    .totals.cases==17 and (.cases|length)==17 and
    (.totals.pass + .totals.mismatch)==17' "$out" >/dev/null; then
    mg_failure compiler "the pinned compiler check failed to produce complete evidence"
    mg_status error; return 0
  fi
  mg_artifact json "$out" "neutral color values and pinned public compiler evidence"
  mg_count total 17
  mg_count passed "$(jq .totals.pass "$out")"
  mg_count failed "$(jq .totals.mismatch "$out")"
  if [ "$rc" -ne 0 ] || [ "$(jq .totals.mismatch "$out")" -ne 0 ]; then
    mg_failure color-text "public compiler output differs from the neutral expectations; see colors.json"
    mg_status fail
  else
    mg_status pass
  fi
}
