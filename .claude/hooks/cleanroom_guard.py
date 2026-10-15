#!/usr/bin/env python3
"""Clean-room guard (KTD1, R1955, R1956) for agent sessions run as subagents.

A subagent started by the orchestrator does not load a worktree's
.claude/settings.local.json, so the implementer deny profile
(.claude/settings.implementer.json) never applies to it. This PreToolUse hook
applies the same rules to subagents whose label marks them as implementers or
fixers (workflow labels `U<n>:implement…`, `U<n>:fix-…`, `U<n>:codexfix-…`;
Agent-tool descriptions starting `implementer` or `fixer`). Every other session,
the main one included, passes through untouched.

The decision is cached per agent id. Errors fail open: the similarity scan and
the clean-room review lens remain the backstop.
"""
import glob
import json
import os
import re
import sys

MAIN = "/Users/antoine/code/meguri"
WORKTREES = MAIN + "-wt"
CACHE = os.path.join(os.environ.get("TMPDIR", "/tmp"), "meguri-cleanroom-guard")

GUARDED_LABEL = re.compile(r"^(U\d+[A-Za-z]?:(implement|fix-|codexfix-)|(implementer|fixer)\b)", re.I)
GUARDED_TEXT = ("You are the IMPLEMENTER", "You are the FIXER", "You are the CODEX FIX RELAY")

# Path fragments an implementer may never touch (any tool, any argument).
PATH_RULES = [
    (re.compile(re.escape(MAIN) + r"/(refs|local-data|oracle)(/|$|\s|['\"])"), "the main checkout's refs/, local-data/ or oracle/"),
    (re.compile(re.escape(WORKTREES) + r"/[^/\s]+/oracle(/|$|\s|['\"])"), "a worktree's oracle/"),
    (re.compile(r"(^|[\s'\"=:(])(\./)?(oracle|local-data)/"), "oracle/ or local-data/ (relative)"),
    (re.compile(r"refs/anki"), "an Anki checkout under refs/"),
    (re.compile(r"oracle-macos-venv"), "the macOS oracle venv"),
    (re.compile(r"build/mg/similarity|build/similarity-index|scan/reviewer\.|\.jplag"), "similarity reviewer views"),
    (re.compile(r"/Applications/Anki\.app|Application(\\ | )Support/Anki2|Anki2/addons21"), "an installed Anki"),
    (re.compile(r"site-packages/(anki|aqt)/"), "the anki/aqt Python packages"),
    (re.compile(r"(github\.com|githubusercontent\.com)/(ankitects|ankidroid|migaku|Migaku)|ankitects/anki|ankidroid/Anki-Android|FooSoft/anki-connect|foosoft\.net|ankiweb\.net/shared"), "Anki source on the web"),
]
# Bash-only rules: reading oracle history, raw docker, installing Anki packages.
BASH_RULES = [
    (re.compile(r"\bgit\b[^|;&]*\bcat-file\b"), "git cat-file"),
    (re.compile(r"\battic/"), "archived attic/ branches (code built outside the clean room)"),
    (re.compile(r"\bdocker(-compose)?\b\s+(exec|cp|run|save|export|build|debug|container)\b|\bdocker-compose\b"), "raw docker (use tools/mg)"),
    (re.compile(r"\b(pip3?|pipx|uv|uvx|python3?\s+-m\s+pip)\b[^|;&]*\b(install|download|wheel|add|tool|run)\b[^|;&]*\b(anki|aqt)\b", re.I), "installing anki/aqt"),
]
# git commands that print file contents. Unit branches carry the spec side's
# oracle/ commits (merged, but kept off disk by the sparse checkout), so these
# must exclude oracle/ explicitly, list names only, or name their own paths.
GIT_CONTENT = re.compile(r"\bgit\b(\s+-C\s+\S+)?(\s+-c\s+\S+)*\s+(diff|show|log|whatchanged|format-patch|blame|stash\s+show|range-diff|difftool)\b")
EXCLUDES_ORACLE = re.compile(r":(\(exclude\)|!|\^)/?oracle")
NAMES_ONLY = re.compile(r"--(stat|shortstat|numstat|name-only|name-status|summary|dirstat)\b|--oneline\b(?!.*(-p|--patch))|--format|--pretty")


def git_reads_oracle(command):
    for part in re.split(r"[|;&]+", command):
        m = GIT_CONTENT.search(part)
        if not m:
            continue
        verb = m.group(3)
        rest = part[m.end():]
        if EXCLUDES_ORACLE.search(rest):
            continue
        if "oracle" in rest:
            return True
        if verb == "log" and not re.search(r"(^|\s)(-p|--patch|-u|--stat|-L)", rest):
            continue  # plain log prints messages only
        if NAMES_ONLY.search(rest) and not re.search(r"(^|\s)(-p|--patch)\b", rest):
            continue
        if " -- " in rest + " ":
            paths = (rest + " ").split(" -- ", 1)[1].split()
            if paths and all(p not in (".", ":/", "*", "./") and not p.startswith(":") for p in paths):
                continue
        return True
    return False


WEB_DENY = re.compile(r"^(https?://)?([^/]*\.)?(github\.com|githubusercontent\.com|ankiweb\.net|foosoft\.net|ankidroid\.org)(/|$)")


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for v in value.values():
            yield from strings(v)
    elif isinstance(value, list):
        for v in value:
            yield from strings(v)


def role_of(data):
    agent = data.get("agent_id")
    if not agent:
        return None
    os.makedirs(CACHE, exist_ok=True)
    cached = os.path.join(CACHE, re.sub(r"[^A-Za-z0-9_-]", "_", agent))
    if os.path.exists(cached):
        with open(cached) as f:
            return f.read().strip()
    session_dir = os.path.splitext(data.get("transcript_path", ""))[0]
    if not session_dir:
        session_dir = os.path.expanduser("~/.claude/projects/*/" + data.get("session_id", "none"))
    stem = agent if agent.startswith("agent-") else "agent-" + agent
    guarded = False
    label = ""
    metas = glob.glob(os.path.join(session_dir, "subagents", "**", stem + ".meta.json"), recursive=True)
    if metas:
        with open(metas[0]) as f:
            label = json.load(f).get("description", "") or ""
        guarded = bool(GUARDED_LABEL.search(label))
    if not guarded:
        transcripts = glob.glob(os.path.join(session_dir, "subagents", "**", stem + ".jsonl"), recursive=True)
        if transcripts:
            with open(transcripts[0]) as f:
                head = f.read(200000)
            guarded = any(t in head for t in GUARDED_TEXT)
        elif not metas:
            return "unknown"  # not written yet: decide again on the next call
    role = "guarded" if guarded else "free"
    with open(cached, "w") as f:
        f.write(role)
    return role


def deny(reason):
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": "Clean room (KTD1, R1955/R1956, docs/process/roles/implementer.md): implementer and fixer sessions may not access "
        + reason + ". Work from specs/, conformance/ and the oracle as a black box through tools/mg. If a spec is missing or unclear, stop and report it (needs_roles).",
    }}))
    sys.exit(0)


PATH_FIELDS = {
    "Read": ("file_path",), "Write": ("file_path",), "Edit": ("file_path",), "MultiEdit": ("file_path",),
    "NotebookEdit": ("notebook_path",), "Grep": ("path", "glob"), "Glob": ("path", "pattern"),
}
RECURSIVE = re.compile(r"(^|[\s;&|(])(rg|ag|ack|fd|tree|find|grep\s+(-\w*[rR]\w*|--recursive)|ls\s+-\w*R)\b")


def at_main_root(path, cwd):
    path = os.path.normpath(os.path.join(cwd or MAIN, path or "."))
    return path == MAIN


HOLD = os.environ.get("MEGURI_HOLD_FILE", os.path.join(MAIN, ".git", "meguri-hold-agents"))  # content: epoch seconds when the hold began
HELD_LOG = os.path.join(MAIN, ".git", "meguri-held-agents.log")


def started_at(data):
    agent = data.get("agent_id", "")
    stem = agent if agent.startswith("agent-") else "agent-" + agent
    session_dir = os.path.splitext(data.get("transcript_path", ""))[0]
    metas = glob.glob(os.path.join(session_dir, "subagents", "**", stem + ".meta.json"), recursive=True)
    if not metas:
        return None
    st = os.stat(metas[0])
    return getattr(st, "st_birthtime", st.st_mtime)


def hold_new_agents(data):
    """Usage pause: agents that started after the hold began wait here, idle, until it is lifted."""
    if not os.path.exists(HOLD):
        return
    try:
        with open(HOLD) as f:
            since = float(f.read().strip() or 0)
    except (OSError, ValueError):
        since = 0
    born = started_at(data)
    if born is None or born < since:
        return  # agents already running when the hold began finish their work
    import time
    with open(HELD_LOG, "a") as f:
        f.write("%d %s %s\n" % (time.time(), data.get("agent_id", ""), data.get("tool_name", "")))
    deadline = time.time() + 540
    while os.path.exists(HOLD) and time.time() < deadline:
        time.sleep(10)
    if os.path.exists(HOLD):
        deny_raw("Paused for Antoine's Claude usage limit: new work waits until it resets. Repeat exactly the same tool call to keep waiting, and do nothing else meanwhile.")


def deny_raw(reason):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}))
    sys.exit(0)


def main():
    data = json.load(sys.stdin)
    if not data.get("agent_id"):
        return
    hold_new_agents(data)
    if role_of(data) != "guarded":
        return
    tool = data.get("tool_name", "")
    tool_input = data.get("tool_input", {}) or {}
    cwd = data.get("cwd") or MAIN
    if tool in ("WebFetch", "WebSearch"):
        url = str(tool_input.get("url", ""))
        if WEB_DENY.search(url):
            deny("Anki-related websites")
    if tool in PATH_FIELDS:
        texts = [str(tool_input.get(k, "")) for k in PATH_FIELDS[tool] if tool_input.get(k)]
        if tool in ("Grep", "Glob") and at_main_root(tool_input.get("path"), cwd):
            deny("a search over the main checkout, which reaches oracle/ and refs/ (search your worktree instead)")
    elif tool == "Bash":
        command = str(tool_input.get("command", ""))
        texts = [command]
        for rule, what in BASH_RULES:
            if rule.search(command):
                deny(what)
        if git_reads_oracle(command):
            deny("oracle/ through git (add -- . ':(exclude)oracle' to git diff/show/log -p, use --stat or --name-only, or name your own paths after --)")
        if os.path.normpath(cwd) == MAIN and RECURSIVE.search(command) and WORKTREES + "/" not in command:
            deny("a recursive search or listing in the main checkout, which reaches oracle/ and refs/ (cd into your worktree first)")
    else:
        texts = []  # reports, structured output, messages and other tools carry text, not file access
    for text in texts:
        for rule, what in PATH_RULES:
            if rule.search(text):
                deny(what)


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception:
        pass
