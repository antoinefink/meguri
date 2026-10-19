# Restricted implementation runner

Run `tools/mg cleanroom-image` after changing the runtime image. Then run
`tools/mg cleanroom --check` to test its boundaries without authentication.

For implementation or fixes, record the appropriate session, commit the input
on master, and run:

```sh
tools/mg cleanroom --prompt <file> --session <session-id>
```

The runner copies tracked, permitted regular files from clean master. It excludes
`refs/`, `oracle/`, `local-data/`, `.git`, all build output and agent configuration,
including nested copies of those names. It refuses symbolic links, hard links
and submodules. It mounts only this copy, its own run directory and its runner
scripts. The container has no host home, Docker socket, shared Cargo cache or
real repository history. Its Git baseline contains only the permitted copy.

Codex uses a command sandbox with networking disabled. Web search, apps, plugins,
hooks and subagents are disabled. The shell cannot read the run directory, which
holds temporary authentication; cleanup removes authentication on success,
failure or cancellation. Every invocation first proves permitted writes, missing
host/source paths, denied credential-directory reads and a denied connection to
a listener proven reachable outside the command sandbox. Probe failure stops the
run. Codex may create an empty `.git` protection directory; the runner accepts
only that empty guard and removes it after the sandbox exits.

The container permits nested unprivileged namespaces for bubblewrap: its Docker
seccomp and system-path masks are disabled, with a read-only
root filesystem and only SETFCAP retained. SETFCAP permits the root UID mapping;
SYS_ADMIN and privileged mode are not used. These options apply only to this
container. The Codex command sandbox remains mandatory.

Review `changes.patch` before applying it to master. The runner never writes to
the real checkout. Run builds and tests through `tools/mg` from the orchestrator;
the restricted runner has no Docker socket or warmed dependency cache. Main
checkout access remains necessary for integration and black-box verification;
its owner must not read forbidden sources or raw similarity reports.

Runtime configuration follows the [OpenAI sandbox documentation](https://developers.openai.com/codex/sandbox)
and [configuration reference](https://developers.openai.com/codex/config-reference).
Isolation does not prove independent authorship. Specs, provenance review
and similarity checks remain required for code and assets.
