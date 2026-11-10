# Cloud agent original process groups: observe, drain, reap

2026-11-10. Implementer view, at `origin/master` `364a4b3c`. This is a design brief only. It adds no contract, waiver, schema or receipt, and it claims no pass. Sources: `tools/scripts/cloud-agent-handoff.py` (`setup_join`, `setup_command`, `Delivery.call`), `cloud-agent-smoke.py`, `cloud-account-setup.py`, `vm-xcode-stage.py` (`join_original`), `specs/conformance/cloudkit-agent.md` J2.2.1, J3.1, J4 and J5.1, the public SDK headers, and the host `/usr/bin/python3` 3.9.6 `subprocess` module.

## Problem as observed

- MG84309: the bridge gets EOF, so `receive()` refuses `transport-ended`. `Bridge.close()` then calls `h.setup_join`. The first child (the original `Popen` process, whose pid is the group id) has exited, but nobody has reaped it yet. `os.killpg(child.pid, SIGTERM)` raises `PermissionError` (EPERM), and that error replaces `transport-ended`. The race reproduces in under 1 s. The already-reaped case and the case where the first child is still alive at close both pass.
- On Darwin, `killpg` returns EPERM when the only member left in the group has exited but is unreaped. Signal 0, SIGTERM and SIGKILL all give EPERM. After the first child is reaped, `killpg` gives ESRCH.
- Second defect: `setup_join` calls `child.wait(timeout=10)`, which may reap the first child, and then calls `killpg(child.pid, SIGKILL)`. After that reap the id is free, so SIGKILL may hit an unrelated group that now uses it.
- The callers reap before cleanup. `Popen.poll`, `wait` and `communicate` all reap. The smoke `run_child` runs `if process.poll() is None: h.setup_join(process)`, so a reaped first child skips the join even when group members are still running. Python 3.9 `_internal_poll` sets `returncode = 0` on ECHILD, which is a false success if anything else reaps the PID first.

## Budgets and fences that stay exactly as they are

- `setup_join`: SIGTERM, then `child.wait(timeout=10)`, then SIGKILL, then `child.wait()` with no limit. Parent SIGINT, SIGTERM and SIGHUP stay `SIG_IGN` during the join ("Finish this join despite parent cancellation.").
- Caller limits: `setup_command` `child.wait(timeout=180)`; `Delivery.call` `child.communicate(input=data, timeout=600)`; smoke `process.wait(timeout=3600)` and `WAIT = 600`; account setup `WAIT` (600); `vm-xcode-stage` `SHUTDOWN = 10` and `COPY_WAIT = 3600`.
- Sampling: managed waits sample in slices of at most 0.2 s inside the caller's existing limit, following the precedent of `communicate_original` (`min(0.2, ...)`). This adds no new time limit.
- Fence: "Only a child launched with start_new_session=True: never a process lookup or a host-wide kill." The only signal call stays `killpg(child_pid, ...)`. The design never calls `kill()` on an enumerated member. The enumeration below is a lookup limited to the pinned group id. That is a deliberate, narrow change to this comment's wording, and the master must accept or reject it.
- Contract: J2.2.1 says "Cancel and join original work before releasing the lease or finalizing the outcome." J3.1 says "process exit or a lost connection alone is not clean close." J4 says "No successor starts while owned work remains active. This adds no timer or repeated execution."

## Core: a managed original Popen

Create a subclass of `subprocess.Popen` and use it at every original-child site that has `start_new_session=True`: `setup_command`, `Delivery.call`, smoke `run_child`, the smoke guest `xcodebuild`, `Bridge`, and the account-setup `xcodebuild`. Construction refuses unless `start_new_session=True`, so pgid == pid. Inside the subclass, the kernel PID stays reserved and unreaped until one final reap. In 3.9.6, `communicate` and `__exit__` complete through the public `wait()`. Override `poll()` and `wait(timeout=None)`, and refuse `send_signal`, `terminate` and `kill`.

### ctypes (arm64 and x86_64 LP64; offsets match MG85496 and a local clang `offsetof` check)

```python
libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
class SigInfo(ctypes.Structure):      # <sys/signal.h> siginfo_t, 104 bytes
    _fields_ = [("si_signo", c_int), ("si_errno", c_int), ("si_code", c_int),   # code @8
                ("si_pid", c_int32), ("si_uid", c_uint32), ("si_status", c_int),  # @12 @16 @20
                ("si_addr", c_void_p), ("si_value", c_void_p), ("si_band", c_long),
                ("pad", c_ulong * 7)]
class BsdInfo(ctypes.Structure):      # <sys/proc_info.h> struct proc_bsdinfo, 136 bytes
    _fields_ = [("pbi_flags", c_uint32), ("pbi_status", c_uint32), ("pbi_xstatus", c_uint32),
                ("pbi_pid", c_uint32), ("pbi_ppid", c_uint32), ("pbi_uid", c_uint32),
                ("pbi_gid", c_uint32), ("pbi_ruid", c_uint32), ("pbi_rgid", c_uint32),
                ("pbi_svuid", c_uint32), ("pbi_svgid", c_uint32), ("rfu_1", c_uint32),
                ("pbi_comm", c_char * 16), ("pbi_name", c_char * 32), ("pbi_nfiles", c_uint32),
                ("pbi_pgid", c_uint32), ("pbi_pjobc", c_uint32), ("e_tdev", c_uint32),
                ("e_tpgid", c_uint32), ("pbi_nice", c_int32),
                ("pbi_start_tvsec", c_uint64), ("pbi_start_tvusec", c_uint64)]
libc.waitid.argtypes = [c_int, c_uint32, POINTER(SigInfo), c_int]; libc.waitid.restype = c_int
libc.proc_listpgrppids.argtypes = [c_int32, c_void_p, c_int]; libc.proc_listpgrppids.restype = c_int
libc.proc_pidinfo.argtypes = [c_int, c_int, c_uint64, c_void_p, c_int]; libc.proc_pidinfo.restype = c_int
```

At import, assert both sizes (104 and 136) and the named offsets. On mismatch, refuse with `implementation_missing`.

Prototypes, with their headers:

- `<sys/wait.h>`: `int waitid(idtype_t, id_t, siginfo_t *, int)`. `idtype_t` is a 4-byte enum. `id_t` is `__uint32_t`. The plain symbol `_waitid` is used on 64-bit.
- `<libproc.h>`: `int proc_listpgrppids(pid_t pgrpid, void *buffer, int buffersize)` and `int proc_pidinfo(int pid, int flavor, uint64_t arg, void *buffer, int buffersize)`.

Constants: `P_PID=1`, `WEXITED=4`, `WNOHANG=1`, `WNOWAIT=32`, `CLD_EXITED=1`, `CLD_KILLED=2`, `CLD_DUMPED=3`, `PROC_PIDTBSDINFO=3`, `SZOMB=5`. The host Python has `os.WNOWAIT` but not `os.waitid`.

### Steps (the same for `poll`, `wait(timeout)` and the join)

1. **Observe without reaping.** Zero a `SigInfo`. Call `waitid(P_PID, pid, &si, WEXITED|WNOWAIT|WNOHANG)`.
   - Retry on EINTR.
   - `0` with `si_pid == 0` means the first child is running. `poll()` returns `None`; `wait()` samples again until its limit and then raises `TimeoutExpired`.
   - `0` with `si_pid == pid` means the first child has an observed exit and pid == pgid stays pinned.
   - Anything else refuses the fence. This includes ECHILD, which means the pin is lost. Never send a signal after this refusal.
2. **Check that the same group is quiet.** Run this only while the first child is pinned. All of the following must hold in one sample:
   - `killpg(pid, 0)` gives EPERM. OK means a live member exists, so sample again. ESRCH while pinned refuses `group-missing`. Any other errno refuses.
   - `proc_listpgrppids(pid, buf, CAP*4)` returns an `r` with `0 < r < CAP`, where CAP is a fixed slot count. `r < 0` refuses `enumeration-error`. `r >= CAP` refuses `enumeration-incomplete`.
   - The listed set is exactly `{pid}`. The first child is accepted only through step 1, because `proc_pidinfo` fails on an exited, unreaped process.
   - For any other listed `q`, `proc_pidinfo(q, 3, 0, &b, 136)`:
     - `136` and `pbi_pgid == pid` and `pbi_uid`, `pbi_ruid` and `pbi_svuid` all equal `getuid()`: a live own member, so sample again.
     - `136` with any of those ids differing: refuse `foreign-member`.
     - `136` with `pbi_pgid != pid`: the PID was reused, so list again.
     - `0` with ESRCH: an exited, unreaped member or one that just left. It is unknown, so sample again. If it is still listed when the caller's limit expires, refuse `unknown-member`.
     - Any other result, such as EPERM: refuse `foreign-member`.
3. **Reap once.** Call `super().wait()`. It returns at once because the first child has already exited. Require `returncode` to match the observed `si_code` and `si_status`: `CLD_EXITED` gives `si_status`, and `CLD_KILLED` or `CLD_DUMPED` gives `-si_status`. A mismatch refuses. After this reap, nothing touches that id: no `kill` or `killpg` of any signal number including 0, and no enumeration.
4. **Completion.** `poll`, `wait` and `communicate` report a return code only after steps 2 and 3. An observed terminal alone is never called joined. After a refusal, nothing is reaped and `returncode` stays `None`.

### `setup_join` in this order (signals stay ignored)

1. If the managed reap has already happened, return. Quiet was proved before that reap.
2. If the first child is not observed exited, or the group is not quiet, send `killpg(pid, SIGTERM)`. EPERM here is not an error: go to step 2 of the core steps. ESRCH refuses `group-missing`.
3. Call the managed `wait(timeout=10)`.
4. On `TimeoutExpired`, the first child is still unreaped and pinned. Send `killpg(pid, SIGKILL)` with the same EPERM and ESRCH handling. Then call the managed `wait()` with no limit, as today.
5. A fence refusal raises one fixed category, `setup-join-uncertain`, never an `OSError`. If a primary exception is in flight (`sys.exc_info()[1]` is set inside `finally`; verified on 3.9.6), that primary keeps its category and the join category goes with it. MG84309 must read `transport-ended`. No lease-safe mark, `joined` frame or successor may follow an uncertain join. `vm-xcode-stage` `release_proof` already does this for `Uncertain`.

## What the headers prove and what they do not

- **Proven by headers and the compiler:** the prototypes, struct fields and types, sizes and offsets, and the constants above.
- **Not proven by headers:**
  - `libproc.h` says "These interfaces are subject to change in future releases." It documents no `proc_listpgrppids` or `proc_pidinfo` semantics, and there is no man page.
  - Return units, zero, failure and overflow come only from a quick local test (python3 -I with `/bin/sh` subprocesses in new sessions). It ran outside MG and proves nothing:
    - The return value is a count of PIDs (1, 2 and 3 in the probe), not bytes.
    - `0` meant no members after the reap.
    - A buffer that was too small returned exactly the slot capacity, with no error. So `r >= CAP` must count as incomplete.
    - A NULL buffer returned an unrelated large number (965), so it cannot size the buffer.
    - The `-1`/errno path was never seen.
  - Exited, unreaped processes, from the same probe:
    - An exited, unreaped first child and an exited, unreaped non-first member both appear in the listing.
    - `proc_pidinfo` (flavor 3 or 13) returns `0` with ESRCH for an exited, unreaped process. Its `pbi_pgid` is therefore unreadable, and this call never reports `SZOMB`.
  - Group pinning: the pgid stays reserved while the first child is exited but unreaped. This follows from the POSIX definitions of process lifetime and process-group lifetime, and from the probe's EPERM before the reap and ESRCH after it. A Darwin reuse test was not possible.
- **Unmeasured:**
  - What happens with a member owned by another uid (`killpg` result, and whether `proc_pidinfo` is visible). This needs a second uid.
  - Whether a listing is atomic with respect to `fork`.
- **Scope limit:** group quiet is not the same as all descendants drained. A descendant that calls `setsid` or `setpgid` to leave the group is not covered, and no claim is made for it.
- **Residual risks:**
  - An exited, unreaped member whose parent is outside the group can stay listed. In the managed `wait()` with no limit after SIGKILL, it holds the join just as the current `child.wait()` would. Bounding this needs a new budget, which the master must decide.
  - Python `__del__` and `_cleanup` can still reap an abandoned instance through `_internal_poll`.
  - `vm-xcode-stage.join_original` has the same EPERM exposure (`except OSError: raise Uncertain()`). It also treats an already-reaped first child as joined (`if child.returncode is not None: return`). It is out of scope here, but the same subclass would fit it.
