#!/bin/bash
# Scratch: design captures on the leased iPhone Duo in the VM.
#   duo.sh launch [fixture] [light|dark] [extra launch args...]
#   duo.sh shot <name> [display]   saves captures/duo/<name>.png
#   duo.sh vm <command>
W=/Users/antoine/.herdr/worktrees/meguri/design
S=/private/tmp/claude-501/-Users-antoine--herdr-worktrees-meguri-design/bcc4eb73-f59e-4573-8583-70b404dcd81d/scratchpad
U=2ACBD139-2FA6-4E7F-A1DF-97ED508483FB
SSH="$(jq -r .ssh "$HOME/Library/Application Support/meguri/mac-vm.json")"
mkdir -p "$S/captures/duo"
case "$1" in
launch)
  fx=${2:-typical}; look=${3:-light}; shift; shift; [ $# -gt 0 ] && shift
  "$SSH" "xcrun simctl ui $U appearance $look" </dev/null
  dir=$W/build/fixtures/recipes/$fx-1
  now=$(python3 -I -c "import json,datetime;m=json.load(open('$dir/manifest.json'));print(datetime.datetime.fromtimestamp(m['now_ms']/1000,datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'))")
  cd "$W" && tools/mg run --platform foldable --device "$U" --config Debug -- -MeguriFixture "$dir" -MeguriNow "$now" -MeguriTimeZone Europe/Paris "$@" | tail -1 | xargs -I{} jq -c '{status,message}' {} | cut -c1-300
  ;;
shot)
  d=${3:-$(cat "$S/rec/position-display" 2>/dev/null || echo 1)}
  "$SSH" "xcrun simctl io $U screenshot --display=$d /tmp/duo-shot.png >/dev/null 2>&1 && cat /tmp/duo-shot.png" </dev/null > "$S/captures/duo/$2.png"
  sips -g pixelWidth -g pixelHeight "$S/captures/duo/$2.png" | tail -2 | tr '\n' ' '; echo "$S/captures/duo/$2.png"
  ;;
position)
  case "$2" in closed) x=2245; d=1;; half) x=2313; d=3;; open) x=2381; d=3;; esac
  printf 'move %s 1889\nclick\n' $x > "$S/rec/vncd.fifo"; echo $d > "$S/rec/position-display"; echo "$2" > "$S/rec/position"; sleep 6
  ;;
tap)
  # tap <x pt> <y pt>, in the current position's display points, through Device Hub's window
  if [ -f "$S/rec/portrait" ]; then ox=1849; oy=704; elif [ "$(cat "$S/rec/position-display")" = 1 ]; then ox=1947; oy=830; else ox=1717; oy=836; fi
  vx=$(python3 -I -c "print(int($ox+$2*0.933))"); vy=$(python3 -I -c "print(int($oy+$3*0.933))")
  printf 'move %s %s\n' $vx $vy > "$S/rec/vncd.fifo"; sleep 0.4; printf 'click\n' > "$S/rec/vncd.fifo"; sleep ${4:-2}
  ;;
hub) printf 'shot %s\n' "$S/rec/hub.png" > "$S/rec/vncd.fifo"; sleep 2 ;;
key) printf 'key %s\n' "$2" > "$S/rec/vncd.fifo"; sleep ${3:-1} ;;
vm) shift; "$SSH" "$*" </dev/null ;;
esac
