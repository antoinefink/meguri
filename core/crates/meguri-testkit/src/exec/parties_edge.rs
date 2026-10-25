//! Native T1 edge leases (sync-ops.md S8.4). Helpers receive transport data
//! only. Diagnostics deliberately contain neither helper output nor argv.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::ExecError;
use super::parties::Route;
use super::sync_guard::check_endpoint;
use crate::wallclock::Stopwatch;

const LIMIT: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn unavailable(detail: &str) -> ExecError {
    ExecError::new("sync_unavailable", detail)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Acquired {
    endpoint: String,
    lease: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Released {
    released: bool,
}

pub(super) struct EdgeLease<'a> {
    command: &'a [String],
    token: Option<String>,
    pub endpoint: String,
}

impl<'a> EdgeLease<'a> {
    pub fn acquire(
        command: &'a [String],
        target: &str,
        route: &Route,
        expected_worker: &str,
    ) -> Result<Self, ExecError> {
        let mut response = Vec::new();
        let invocation = invoke_into(
            command,
            &json!({"cmd": "acquire", "target": target, "route": route, "expected_worker": expected_worker}),
            TIMEOUT,
            &mut response,
        );
        let value = serde_json::from_slice::<Value>(&response).unwrap_or(Value::Null);
        // Retain a returned token even if another member is malformed, so
        // that a rejected response cannot orphan a known allocation.
        let mut lease = Self {
            command,
            token: value
                .get("lease")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            endpoint: String::new(),
        };
        let parsed = invocation
            .and_then(|()| {
                serde_json::from_slice::<Acquired>(&response).map_err(|_| unavailable("invalid edge acquire response"))
            })
            .and_then(|r| {
                if r.endpoint.is_empty() || r.lease.is_empty() {
                    Err(unavailable("empty edge acquire response"))
                } else {
                    Ok(r)
                }
            });
        match parsed {
            Ok(response) => {
                lease.endpoint = response.endpoint;
                Ok(lease)
            }
            Err(mut error) => {
                if let Err(cleanup) = lease.release() {
                    error.detail.push_str(&format!("; cleanup: {cleanup}"));
                }
                Err(error)
            }
        }
    }

    pub fn check_endpoint(&self) -> Result<(), ExecError> {
        check_endpoint(&self.endpoint)
            .map_err(|_| ExecError::new("endpoint_refused", "the meguri edge endpoint was refused (R1876)"))?;
        let port = self
            .endpoint
            .strip_prefix("http://127.0.0.1:")
            .and_then(|s| s.strip_suffix('/'));
        if !port.is_some_and(|p| {
            !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n != 0)
        }) {
            return Err(ExecError::new(
                "endpoint_refused",
                "the meguri edge must be a root HTTP loopback URL with an explicit port",
            ));
        }
        Ok(())
    }

    pub fn release(&mut self) -> Result<(), ExecError> {
        let Some(token) = self.token.take() else { return Ok(()) };
        let response = invoke(self.command, &json!({"cmd": "release", "lease": token}))?;
        let response: Released =
            serde_json::from_slice(&response).map_err(|_| unavailable("invalid edge release response"))?;
        if !response.released {
            return Err(unavailable("edge release failed"));
        }
        Ok(())
    }
}

impl Drop for EdgeLease<'_> {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

/// Kill the entire invocation group and reap the direct child on all exits.
/// MG's run cancellation also removes labeled pre-token resources on the host.
struct Invocation(Child);

impl Drop for Invocation {
    fn drop(&mut self) {
        // SAFETY: the child starts a new process group whose id is its pid.
        unsafe {
            libc::kill(-(self.0.id() as libc::pid_t), libc::SIGKILL);
        }
        let _ = self.0.wait();
    }
}

fn nonblocking(fd: std::os::fd::RawFd) -> Result<(), ExecError> {
    // SAFETY: fcntl changes flags on a live pipe owned by this invocation.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(unavailable("edge helper pipe setup failed"));
    }
    Ok(())
}

fn invoke(command: &[String], request: &Value) -> Result<Vec<u8>, ExecError> {
    invoke_bounded(command, request, TIMEOUT)
}

fn invoke_bounded(command: &[String], request: &Value, timeout: Duration) -> Result<Vec<u8>, ExecError> {
    let mut output = Vec::new();
    invoke_into(command, request, timeout, &mut output)?;
    Ok(output)
}

fn invoke_into(command: &[String], request: &Value, timeout: Duration, output: &mut Vec<u8>) -> Result<(), ExecError> {
    let Some((program, args)) = command.split_first() else {
        return Err(unavailable("empty edge helper command"));
    };
    let mut input = serde_json::to_vec(request).map_err(|_| unavailable("edge request encoding failed"))?;
    input.push(b'\n');
    if input.len() > LIMIT {
        return Err(unavailable("edge request exceeds limit"));
    }
    let child = Command::new(program)
        .args(args)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| unavailable("edge helper could not start"))?;
    let mut child = Invocation(child);
    let stdin = child
        .0
        .stdin
        .take()
        .ok_or_else(|| unavailable("edge helper stdin missing"))?;
    let mut stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| unavailable("edge helper stdout missing"))?;
    nonblocking(stdin.as_raw_fd())?;
    nonblocking(stdout.as_raw_fd())?;
    let mut stdin = Some(stdin);
    let mut written = 0;
    let timer = Stopwatch::start();
    let mut exited = None;
    let mut eof = false;
    while timer.elapsed() < timeout {
        if let Some(pipe) = stdin.as_mut() {
            match pipe.write(&input[written..]) {
                Ok(0) => return Err(unavailable("edge helper stdin closed")),
                Ok(n) => written += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(unavailable("edge helper request failed")),
            }
            if written == input.len() {
                stdin = None;
            }
        }
        let mut bytes = [0; 4096];
        match stdout.read(&mut bytes) {
            Ok(0) => eof = true,
            Ok(n) => output.extend_from_slice(&bytes[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err(unavailable("edge helper response failed")),
        }
        if output.len() > LIMIT {
            return Err(unavailable("edge helper response exceeds limit"));
        }
        if exited.is_none() {
            exited = child.0.try_wait().map_err(|_| unavailable("edge helper wait failed"))?;
        }
        if let Some(status) = exited.as_ref() {
            if !status.success() {
                return Err(unavailable("edge helper returned a failing exit code"));
            }
            if eof {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(unavailable("edge helper timed out"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_helper_is_killed_and_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let command = vec![
            "python3".into(),
            "-I".into(),
            "-B".into(),
            "-c".into(),
            "import os,pathlib,sys,time; pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(120)"
                .into(),
            pid_file.display().to_string(),
        ];
        let error = invoke_bounded(
            &command,
            &json!({"cmd":"acquire","target":"http://sync-faults:30001/"}),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(error.kind, "sync_unavailable");
        assert!(error.detail.contains("timed out"));
        let pid: libc::pid_t = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        // SAFETY: signal zero observes existence without signalling the pid.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }
}
