#!/usr/bin/env python3
"""Contact sheets and motion findings from a screen recording (U8, R1976).

For each marked window of the video (a transition, by default the whole
video) it samples evenly spaced instants, shows the frame on screen at each
(the last frame at or before it, so variable-frame-rate recordings from
`simctl io recordVideo` sample correctly) and tiles them, each labelled with
its frame time, into <out>/<window>.png. It also reports, as data for the
critic (U10):

- jump cuts: frames whose scene-change score (ffmpeg scdet, 0-100) reaches the
  threshold, i.e. the picture changed abruptly instead of animating;
- stalls: within a window, a pause of at least the stall time between two
  changes of the picture (motion that stops and starts again). Stillness
  before the first change or after the last one is not a stall.

With --fit, each marked window is only where to look: the sheet spans the
frames that change inside it, from the still frame before the first change to
the last change, so the tiles show the motion rather than the stillness
around it. A window without any change is kept as it is.

usage: contact_sheet.py <video> --out <dir> [--window <name>=<start>-<end>]...
         [--marks <marks.json>] [--fit] [--frames 12] [--columns 4]
         [--cut-threshold 10] [--stall-seconds 0.25]
marks.json: {"windows": [{"name": "...", "start": <s>, "end": <s>}, ...]},
times in seconds from the start of the video. Writes <out>/report.json.
Exit code: 0 done (findings are data, not failures), 2 bad input.

Runs in the image-tools container (ffmpeg, ffprobe, DejaVu fonts).
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

TILE_WIDTH = 320
MARGIN = 8
PADDING = 8
BACKGROUND = "0x808080"
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
# A frame whose mean absolute difference from the one before (scdet's mafd,
# 0-100) exceeds this changed the picture.
CHANGE_EPSILON = 0.1
TIME_EPSILON = 1e-6
# With --fit, the sheet starts this long before the first change, so its
# first tile shows the still frame before the motion.
FIT_LEAD = 1 / 60
NAME = re.compile(r"^[a-z0-9][a-z0-9-]*$")


class InputError(Exception):
    pass


def ffprobe_stream(video):
    result = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0",
                             "-show_entries", "stream=width,height:format=duration", "-of", "json", video],
                            capture_output=True, text=True)
    if result.returncode != 0:
        raise InputError("ffprobe cannot read %s: %s" % (video, result.stderr.strip()))
    data = json.loads(result.stdout)
    if not data.get("streams"):
        raise InputError("%s has no video stream" % video)
    stream = data["streams"][0]
    return int(stream["width"]), int(stream["height"]), float(data.get("format", {}).get("duration") or 0)


def read_frames(video):
    """Every frame as (index, time, mafd, score), from one scdet pass."""
    result = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-v", "error", "-i", video, "-an",
                             "-vf", "scdet=threshold=100,metadata=print:file=-", "-f", "null", "-"],
                            capture_output=True, text=True)
    if result.returncode != 0:
        raise InputError("ffmpeg cannot decode %s: %s" % (video, result.stderr.strip()))
    out, current = [], None
    for line in result.stdout.splitlines():
        m = re.match(r"^frame:(\d+)\s+pts:\S+\s+pts_time:(\S+)", line)
        if m:
            current = {"frame": int(m.group(1)), "time": float(m.group(2)), "mafd": 0.0, "score": 0.0}
            out.append(current)
        elif current is not None and line.startswith("lavfi.scd.mafd="):
            current["mafd"] = float(line.split("=", 1)[1])
        elif current is not None and line.startswith("lavfi.scd.score="):
            current["score"] = float(line.split("=", 1)[1])
    if not out:
        raise InputError("%s has no frames" % video)
    return out


def on_screen(all_frames, t):
    """The frame showing at time t: the last one at or before it."""
    shown = all_frames[0]
    for f in all_frames:
        if f["time"] <= t + TIME_EPSILON:
            shown = f
        else:
            break
    return shown


def changes(all_frames, start, end):
    """Times of the frames in [start, end] that differ from the one before."""
    return [f["time"] for f in all_frames
            if f["frame"] > 0 and f["mafd"] > CHANGE_EPSILON and start - TIME_EPSILON <= f["time"] <= end + TIME_EPSILON]


def stalls(all_frames, start, end, stall_seconds):
    changes_ = changes(all_frames, start, end)
    found = []
    for a, b in zip(changes_, changes_[1:]):
        if b - a >= stall_seconds - TIME_EPSILON:
            found.append({"start": round(a, 4), "end": round(b, 4), "duration": round(b - a, 4)})
    return found


def tile_sheet(video, picks, out_png, columns, width, height, work, step):
    """Extracts each picked frame once and tiles them, each labelled with its
    offset from the window start (tile i was sampled at i * step)."""
    unique = sorted({p["frame"] for p in picks})
    expr = "+".join("eq(n,%d)" % n for n in unique)
    tile_h = TILE_WIDTH * height // width
    tile_h -= tile_h % 2
    label = ("drawtext=fontfile=%s:text='+%%{eif\\:n*%d\\:d} ms':x=6:y=6:fontsize=16:fontcolor=white:"
             "box=1:boxcolor=black@0.6:boxborderw=4" % (FONT, round(step * 1000)))
    pattern = os.path.join(work, "frame-%04d.png")
    subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-i", video, "-an",
                    "-vf", "select='%s',scale=%d:%d" % (expr, TILE_WIDTH, tile_h),
                    "-fps_mode", "passthrough", pattern], check=True)
    files = {n: os.path.join(work, "frame-%04d.png" % (i + 1)) for i, n in enumerate(unique)}
    for i, p in enumerate(picks):
        shutil.copyfile(files[p["frame"]], os.path.join(work, "tile-%04d.png" % (i + 1)))
    rows = -(-len(picks) // columns)
    subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-y", "-framerate", "1",
                    "-i", os.path.join(work, "tile-%04d.png"),
                    "-vf", "%s,tile=%dx%d:margin=%d:padding=%d:color=%s" % (label, columns, rows, MARGIN, PADDING, BACKGROUND),
                    "-frames:v", "1", out_png], check=True)


def run(video, out, windows=None, frames=12, columns=4, cut_threshold=10.0, stall_seconds=0.25, fit=False):
    """Writes the sheets and returns the report (also written as report.json)."""
    count = frames
    if count < 2 or columns < 1:
        raise InputError("need at least 2 frames per sheet and 1 column")
    if not os.path.isfile(video):
        raise InputError("no video at %s" % video)
    width, height, duration = ffprobe_stream(video)
    all_frames = read_frames(video)
    last = all_frames[-1]["time"]
    duration = max(duration, last)
    if not windows:
        windows = [("video", 0, duration)]
    for name, start, end in windows:
        if not NAME.match(name):
            raise InputError("window name '%s': use lowercase letters, digits and '-'" % name)
        if not (0 <= start < end):
            raise InputError("window %s: needs 0 <= start < end, got %s-%s" % (name, start, end))
        if start > duration:
            raise InputError("window %s starts at %s s, after the video ends (%.3f s)" % (name, start, duration))
    os.makedirs(out, exist_ok=True)
    cuts = [{"frame": f["frame"], "time": round(f["time"], 4), "score": f["score"]}
            for f in all_frames if f["frame"] > 0 and f["score"] >= cut_threshold]
    report = {
        "video": {"path": video, "width": width, "height": height, "duration": round(duration, 4), "frames": len(all_frames)},
        "settings": {"frames": count, "columns": columns, "cut_threshold": cut_threshold, "stall_seconds": stall_seconds},
        "jump_cuts": cuts,
        "windows": [],
    }
    for name, start, end in windows:
        end = min(end, duration)
        searched = {"start": start, "end": end}
        moving = changes(all_frames, start, end) if fit else []
        if moving:
            start, end = max(searched["start"], moving[0] - FIT_LEAD), moving[-1]
        step = (end - start) / (count - 1)
        times = [start + i * step for i in range(count)]
        picks = []
        for i, t in enumerate(times):
            shown = on_screen(all_frames, t)
            picks.append({"index": i, "time": round(t, 4), "frame": shown["frame"], "frame_time": round(shown["time"], 4)})
        sheet = os.path.join(out, name + ".png")
        with tempfile.TemporaryDirectory() as work:
            tile_sheet(video, picks, sheet, columns, width, height, work, step)
        report["windows"].append({
            "name": name,
            "start": round(start, 4),
            "end": round(end, 4),
            "searched": searched,
            "sheet": sheet,
            "tiles": picks,
            "jump_cuts": [c for c in cuts if searched["start"] < c["time"] <= searched["end"] + TIME_EPSILON],
            "stalls": stalls(all_frames, searched["start"], searched["end"], stall_seconds),
        })
    with open(os.path.join(out, "report.json"), "w") as f:
        json.dump(report, f, indent=2)
    return report


def parse_window(text):
    m = re.match(r"^([^=]+)=([0-9.]+)-([0-9.]+)$", text)
    if not m:
        raise InputError("--window takes <name>=<start>-<end> in seconds, got '%s'" % text)
    try:
        return m.group(1), float(m.group(2)), float(m.group(3))
    except ValueError:
        raise InputError("--window takes <name>=<start>-<end> in seconds, got '%s'" % text)


def main(argv):
    parser = argparse.ArgumentParser(description="Contact sheets, jump cuts and stalls from a recording (U8).")
    parser.add_argument("video")
    parser.add_argument("--out", required=True)
    parser.add_argument("--window", action="append", default=[])
    parser.add_argument("--marks")
    parser.add_argument("--fit", action="store_true")
    parser.add_argument("--frames", type=int, default=12)
    parser.add_argument("--columns", type=int, default=4)
    parser.add_argument("--cut-threshold", type=float, default=10.0)
    parser.add_argument("--stall-seconds", type=float, default=0.25)
    args = parser.parse_args(argv)
    try:
        windows = [parse_window(w) for w in args.window]
        if args.marks:
            with open(args.marks) as f:
                marks = json.load(f)
            for w in marks.get("windows", []):
                windows.append((str(w["name"]), float(w["start"]), float(w["end"])))
        report = run(args.video, args.out, windows, frames=args.frames, columns=args.columns,
                     cut_threshold=args.cut_threshold, stall_seconds=args.stall_seconds, fit=args.fit)
    except (InputError, OSError, ValueError, KeyError, TypeError) as error:
        print("contact-sheet: %s" % error, file=sys.stderr)
        return 2
    except subprocess.CalledProcessError as error:
        print("contact-sheet: %s failed with exit code %d" % (error.cmd[0], error.returncode), file=sys.stderr)
        return 2
    for w in report["windows"]:
        print("contact-sheet: %s: %s (%d tiles, %d jump cut(s), %d stall(s))"
              % (w["name"], w["sheet"], len(w["tiles"]), len(w["jump_cuts"]), len(w["stalls"])), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
