//! The oracle executor: a pool of long-lived oracle workers keyed by time zone
//! (one zone per worker, R1871), speaking JSON lines over stdio
//! (conformance/schema/protocol.json). `tools/mg conformance --live` passes a
//! worker command that starts the oracle image through `docker compose run -i`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Condvar, Mutex, MutexGuard};

use serde_json::{Map, Value, json};

use super::{ExecError, Executor, Needs};
use crate::scenario::LoadedScenario;

/// A worker's hello response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub anki_version: String,
    pub lock_sha256: String,
    pub driver_sha256: String,
    pub tz: String,
}

impl Hello {
    fn from_response(response: &Value) -> Option<Hello> {
        let field = |k: &str| response.get(k)?.as_str().map(str::to_owned);
        Some(Hello {
            anki_version: field("anki_version")?,
            lock_sha256: field("lock_sha256")?,
            driver_sha256: field("driver_sha256")?,
            tz: field("tz")?,
        })
    }
}

#[derive(Debug)]
struct Worker {
    tz: String,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Worker {
    fn start(command: &[String], tz: &str) -> Result<Worker, ExecError> {
        let args: Vec<String> = command.iter().map(|a| a.replace("{tz}", tz)).collect();
        let (program, rest) = args
            .split_first()
            .ok_or_else(|| ExecError::new("worker_start", "empty worker command"))?;
        let mut child = Command::new(program)
            .args(rest)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ExecError::new("worker_start", format!("cannot start {program}: {e}")))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(ExecError::new("worker_start", "the worker has no stdio pipes"));
        };
        Ok(Worker {
            tz: tz.to_owned(),
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        })
    }

    /// One request line, one response line.
    fn request(&mut self, request: &Value) -> Result<Value, ExecError> {
        let died = |why: String| ExecError::new("worker_died", why);
        let mut line = serde_json::to_string(request).map_err(|e| ExecError::new("internal_error", e.to_string()))?;
        line.push('\n');
        let stdin = self.stdin.as_mut().ok_or_else(|| died("stdin closed".to_owned()))?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|e| died(format!("writing: {e}")))?;
        let mut response = String::new();
        let n = self
            .stdout
            .read_line(&mut response)
            .map_err(|e| died(format!("reading: {e}")))?;
        if n == 0 {
            let status = self
                .child
                .try_wait()
                .ok()
                .flatten()
                .map(|s| s.to_string())
                .unwrap_or_default();
            return Err(died(format!("the worker exited {status}").trim_end().to_owned()));
        }
        serde_json::from_str(&response).map_err(|_| died(format!("non-JSON line: {:.200}", response.trim_end())))
    }
}

impl Drop for Worker {
    /// A worker stops at the end of its stdin; close it and reap the process.
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[derive(Debug, Default)]
struct Pool {
    idle: Vec<Worker>,
    /// Workers started and not yet stopped, idle or busy.
    live: usize,
}

#[derive(Debug)]
pub struct OracleExecutor {
    command: Vec<String>,
    max_workers: usize,
    pool: Mutex<Pool>,
    freed: Condvar,
    hello: Mutex<Option<Hello>>,
}

impl OracleExecutor {
    /// `command` starts one worker; `{tz}` in any argument becomes its zone.
    /// At most `max_workers` run at once.
    pub fn new(command: Vec<String>, max_workers: usize) -> OracleExecutor {
        OracleExecutor {
            command,
            max_workers: max_workers.max(1),
            pool: Mutex::new(Pool::default()),
            freed: Condvar::new(),
            hello: Mutex::new(None),
        }
    }

    /// The first hello a worker answered.
    pub fn hello(&self) -> Option<Hello> {
        self.hello.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn lock(&self) -> MutexGuard<'_, Pool> {
        self.pool.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// An idle worker of the zone, a new one, or (when the pool is full) the
    /// place of an idle worker of another zone; otherwise waits.
    fn checkout(&self, tz: &str) -> Result<Worker, ExecError> {
        let mut pool = self.lock();
        loop {
            if let Some(i) = pool.idle.iter().position(|w| w.tz == tz) {
                return Ok(pool.idle.swap_remove(i));
            }
            if pool.live < self.max_workers {
                pool.live += 1;
                drop(pool);
                return self.start(tz).inspect_err(|_| self.release_slot());
            }
            if let Some(i) = pool.idle.iter().position(|w| w.tz != tz) {
                let evicted = pool.idle.swap_remove(i);
                pool.live -= 1;
                drop(pool);
                drop(evicted);
                pool = self.lock();
                continue;
            }
            pool = self.freed.wait(pool).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn start(&self, tz: &str) -> Result<Worker, ExecError> {
        let mut worker = Worker::start(&self.command, tz)?;
        let response = worker.request(&json!({"cmd": "hello"}))?;
        let hello = Hello::from_response(&response)
            .filter(|_| response.get("ok") == Some(&Value::Bool(true)))
            .ok_or_else(|| ExecError::new("worker_start", format!("no hello from the worker: {response}")))?;
        self.hello
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(hello);
        Ok(worker)
    }

    fn checkin(&self, worker: Worker) {
        self.lock().idle.push(worker);
        self.freed.notify_all();
    }

    fn release_slot(&self) {
        let mut pool = self.lock();
        pool.live = pool.live.saturating_sub(1);
        drop(pool);
        self.freed.notify_all();
    }
}

impl Executor for OracleExecutor {
    fn name(&self) -> &str {
        "oracle"
    }

    /// The oracle reports what it cannot do itself (unsupported_op and the
    /// like), so nothing is pending before a run.
    fn missing(&self, _needs: &Needs) -> Needs {
        Needs::default()
    }

    fn run(&self, loaded: &LoadedScenario) -> Result<Value, ExecError> {
        let mut worker = self.checkout(&loaded.scenario.clock.tz)?;
        let request = json!({"cmd": "run", "id": loaded.scenario.id, "scenario": loaded.value});
        let response = match worker.request(&request) {
            Ok(response) => response,
            Err(e) => {
                drop(worker);
                self.release_slot();
                return Err(e);
            }
        };
        self.checkin(worker);
        if response.get("ok") == Some(&Value::Bool(true)) {
            return response
                .get("observations")
                .cloned()
                .ok_or_else(|| ExecError::new("protocol_error", "a run response without observations"));
        }
        Err(protocol_error(response.get("error").and_then(Value::as_object)))
    }
}

/// A structured error response (protocol.json `error`).
fn protocol_error(error: Option<&Map<String, Value>>) -> ExecError {
    let Some(error) = error else {
        return ExecError::new("protocol_error", "an error response without an error");
    };
    let kind = error.get("kind").and_then(Value::as_str).unwrap_or("unknown");
    let mut fields: Vec<String> = error
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "kind" | "step"))
        .map(|(k, v)| format!("{k}={}", v.as_str().map_or_else(|| v.to_string(), str::to_owned)))
        .collect();
    fields.sort();
    let mut e = ExecError::new(kind, fields.join(" "));
    if let Some(step) = error
        .get("step")
        .and_then(Value::as_u64)
        .and_then(|s| usize::try_from(s).ok())
    {
        e = e.at_step(step);
    }
    e
}
