# The development identity's app icon. Sourced by tools/mg.
#
# app/Meguri/AppIcon-Dev.icon is generated from app/Meguri/AppIcon.icon by
# tools/scripts/dev-icon/dev_icon.py (a hue change only: the red becomes the
# system blue family). Development builds select it in app/Config (the
# identity's app icon name); xcode-check checks the built products.

mg_job dev-icon docker:image-tools mg_dev_icon \
  "Development app icon: generate (default) rewrites app/Meguri/AppIcon-Dev.icon from AppIcon.icon by a hue change (red to system blue); check fails when it is stale; analyze <png>... prints hue histograms"

# tools/mg dev-icon [generate|check|analyze <png>...]
mg_dev_icon() {
  local mode="${1:-generate}" out="$MG_RUN_DIR/dev-icon.txt" rc
  [ "$#" -eq 0 ] || shift
  case "$mode" in
    generate | check) [ "$#" -eq 0 ] || mode=usage ;;
    analyze) [ "$#" -gt 0 ] || mode=usage ;;
    *) mode=usage ;;
  esac
  if [ "$mode" = usage ]; then
    mg_failure usage "usage: tools/mg dev-icon [generate|check|analyze <png>...]" "tools/mg dev-icon"
    mg_status error
    return 0
  fi
  python3 tools/scripts/dev-icon/dev_icon.py "$mode" ${1+"$@"} >"$out" 2>&1
  rc=$?
  cat "$out" >&2
  mg_artifact log "$out" "dev_icon.py output"
  case "$rc" in
    0)
      mg_message "dev-icon: $mode done"
      mg_status pass ;;
    1)
      mg_failure dev-icon-stale "$(tail -n 1 "$out")" "tools/mg dev-icon generate"
      mg_status fail ;;
    *)
      mg_failure dev-icon "$(tail -n 1 "$out")"
      mg_status error ;;
  esac
}
