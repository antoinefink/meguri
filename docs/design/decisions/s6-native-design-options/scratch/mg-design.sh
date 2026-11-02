#!/bin/zsh
# Scratch: launch the isolated hidden design build and capture its windows.
#   mg-design.sh launch [fixture] [WxH] [light|dark]
#   mg-design.sh send <command>
#   mg-design.sh shot <name>     captures every window of the app to captures/<name>-<n>.png
#   mg-design.sh quit
W=/Users/antoine/.herdr/worktrees/meguri/design
S=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
APP=$W/build/app/DesignDD/Build/Products/Debug/Meguri.app
ID=com.antoinefink.meguri.design.dev
mkdir -p $S/captures
pid() { pgrep -f "$APP/Contents/MacOS/Meguri" | head -1; }
case "$1" in
launch)
  [ -n "$(pid)" ] && kill "$(pid)" && sleep 1
  fx=${2:-typical}; size=${3:-1180x780}; look=${4:-light}; extra=(${=5})
  dir=$W/build/fixtures/recipes/$fx-1
  now=$(python3 -c "import json,datetime;m=json.load(open('$dir/manifest.json'));print(datetime.datetime.fromtimestamp(m['now_ms']/1000,datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))")
  open -g -j -n "$APP" --args -MeguriBackgroundLaunch YES -MeguriFixture "$dir" -MeguriNow "$now" -MeguriTimeZone Europe/Paris -MeguriWindowSize "$size" -MeguriAppearance "$look" $extra
  sleep 3; $S/mgax post "$ID.design" unhide; sleep 0.3; $S/mgax post "$ID.design" offscreen; sleep 0.5; pid
  ;;
route)
  [ -n "$(pid)" ] && kill "$(pid)" && sleep 1
  open -g -j -n "$APP" --args -MeguriBackgroundLaunch YES -MeguriRoute "$2" -MeguriWindowSize "${3:-1180x780}" -MeguriAppearance "${4:-light}"
  sleep 3; $S/mgax windows "$(pid)"
  ;;
send) $S/mgax post "$ID.design" "$2"; sleep ${3:-1.5} ;;
shot)
  $S/mgax post "$ID.design" "shot:$2"; sleep 1.5
  for f in ~/Library/Containers/$ID/Data/tmp/captures/$2-*.png; do cp "$f" $S/captures/ && echo $S/captures/${f:t}; done
  ;;
quit) [ -n "$(pid)" ] && kill "$(pid)" ;;
esac
