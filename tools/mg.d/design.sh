# Design documents (U9, R2001). Sourced by tools/mg.
#
# docs/design/screen-inventory.yaml is the only screen inventory; screens.md
# is generated from it (tools/scripts/design-screens.py). Runs in the dev
# container (PyYAML).

mg_job design-docs docker:dev mg_design_docs \
  "Design documents (R2001): validate docs/design/screen-inventory.yaml and check that screens.md is generated from it; --write regenerates screens.md"

# tools/mg design-docs [--write]
mg_design_docs() {
  local mode=check
  case "${1:-}" in
    "") ;;
    --write) mode=write ;;
    *)
      mg_failure usage "unknown argument '$1'; usage: tools/mg design-docs [--write]" "tools/mg design-docs"
      mg_status error
      return 0
      ;;
  esac
  local out="$MG_RUN_DIR/design-screens.txt" rc
  python3 tools/scripts/design-screens.py "$mode" >"$out" 2>&1
  rc=$?
  cat "$out" >&2
  if [ "$rc" -eq 0 ]; then
    mg_artifact markdown docs/design/screens.md "screens.md"
    mg_message "design-docs: inventory valid, screens.md $([ "$mode" = write ] && echo written || echo current)"
    mg_status pass
    return 0
  fi
  local line
  while IFS= read -r line; do
    [ -n "$line" ] && mg_failure "design-docs/${line%%:*}" "$line"
  done <"$out"
  mg_message "design-docs: $(grep -c . "$out") problem(s)"
  mg_status fail
}
