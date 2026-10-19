# Spec-side generation in the oracle image's pinned Python runtime.
# Outputs stay in the run directory; recording/publishing is a separate step.
mg_job corpus-generate host mg_corpus_generate \
  "Generate the type-answer comparison corpus with pinned Python: type-answer; JSON artifacts only under the run directory"

mg_corpus_generate() {
  if [ "$#" -ne 1 ] || [ "${1:-}" != type-answer ]; then
    mg_failure usage "usage: tools/mg corpus-generate type-answer"
    mg_status error
    return 0
  fi
  local source="$MG_ROOT/conformance/corpora/type-answer/generate.py"
  local out="$MG_RUN_DIR/generated" file n=0
  if [ ! -f "$source" ] || [ -L "$source" ]; then
    mg_failure generator "the tracked type-answer generator must be a regular file"
    mg_status error
    return 0
  fi
  mg_oracle__tree || return 0
  mg_oracle__build "$MG_ORACLE_DIR" || return 0
  mkdir -p "$out" || return 1
  if ! mg_oracle__run "$MG_ORACLE_TAG" "$MG_ORACLE_DIR" \
    -v "$source:/generator.py:ro" -v "$out:/out" --entrypoint python3 oracle \
    -I -B -c 'import json, runpy, sys, unicodedata; assert sys.version_info[:2] == (3, 12), "type-answer generation requires pinned Python 3.12"; sys.argv = [sys.argv[1], "--out", sys.argv[2]]; runpy.run_path(sys.argv[0], run_name="__main__"); print(json.dumps({"python": sys.version.split()[0], "unicode": unicodedata.unidata_version}))' \
    /generator.py /out >"$MG_RUN_DIR/runtime.json"; then
    mg_failure corpus-generate "the pinned generator failed; no scenario files were published"
    mg_status error
    return 0
  fi
  if ! jq -e '.python | startswith("3.12.")' "$MG_RUN_DIR/runtime.json" >/dev/null || \
    ! jq -e '.unicode | type=="string" and length>0' "$MG_RUN_DIR/runtime.json" >/dev/null; then
    mg_failure corpus-runtime "the generator did not report its pinned runtime"
    mg_status error
    return 0
  fi
  for file in 01 02 03 04 05 06 07 08 09 10; do
    if [ ! -f "$out/compare-$file.json" ] || ! jq -e . "$out/compare-$file.json" >/dev/null; then
      mg_failure corpus-output "the generator did not produce ten readable JSON scenarios"
      mg_status error
      return 0
    fi
    n=$((n + 1))
  done
  mg_count total "$n"
  mg_count passed "$n"
  mg_detail runtime "$(cat "$MG_RUN_DIR/runtime.json")"
  mg_detail generator "$(jq -n --arg path conformance/corpora/type-answer/generate.py \
    --arg sha256 "$(mg_oracle__sha256 <"$source")" '{path:$path, sha256:$sha256}')"
  mg_artifact runtime "$MG_RUN_DIR/runtime.json" "Python and Unicode versions"
  mg_artifact corpus "$out" "generated comparison scenarios; not published"
  mg_message "corpus-generate: generated $n scenarios under the run directory"
  mg_status pass
}
