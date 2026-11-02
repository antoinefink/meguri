#!/bin/zsh
# Opens the prototype on the synthetic fixture, in its own container.
W=/Users/antoine/.herdr/worktrees/meguri/design
APP=$W/build/app/DesignDD/Build/Products/Debug/Meguri.app
pkill -f "$APP/Contents/MacOS/Meguri"; sleep 1
open -n "$APP" --args -MeguriFixture "$W/build/fixtures/recipes/review-ready-1" -MeguriWindowSize 1180x780
