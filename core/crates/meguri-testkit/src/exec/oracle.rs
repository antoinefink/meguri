//! The oracle executor: a pool of long-lived oracle workers keyed by time zone
//! (one zone per worker, R1871), speaking JSON lines over stdio
//! (conformance/schema/protocol.json). `tools/mg conformance --live` passes a
//! worker command that starts the oracle image through `docker compose run -i`,
//! with the handoff directory of live chains mounted.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Condvar, Mutex, MutexGuard};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{ExecError, Executor, Handoff, Interactive, Needs, SegmentExecutor, SegmentOutput, SegmentRequest};
use crate::observe;
use crate::scenario::LoadedScenario;
use crate::suite::OraclePin;

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

// Validate duplicate route fields before the JSON Value representation can
// collapse them. Parties still receives a successful begin with malformed
// sync, so its guard attempts end before quarantining the worker.
#[derive(Deserialize)]
struct BeginWire {
    #[serde(rename = "sync")]
    _sync: RouteWire,
}

#[derive(Deserialize)]
struct RouteWire {
    #[serde(rename = "route", deserialize_with = "Option::deserialize")]
    _route: Option<super::parties::Route>,
}

#[derive(Debug)]
struct Worker {
    tz: String,
    identity: Option<String>,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Worker {
    fn start(command: &[String], tz: &str) -> Result<Worker, ExecError> {
        // Only a factory command explicitly requesting the identity establishes
        // trusted context. Assign it before spawning, outside protocol data.
        let identity = if command.iter().any(|a| a.contains("{worker}")) {
            let mut bytes = [0_u8; 16];
            std::fs::File::open("/dev/urandom")
                .and_then(|mut f| f.read_exact(&mut bytes))
                .map_err(|_| ExecError::new("worker_start", "worker identity unavailable"))?;
            Some(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
        } else {
            None
        };
        let args: Vec<String> = command
            .iter()
            .map(|a| {
                a.replace("{tz}", tz)
                    .replace("{worker}", identity.as_deref().unwrap_or(""))
            })
            .collect();
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
            identity,
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
        let mut value: Value =
            observe::parse_json(&response).map_err(|_| died("worker returned non-JSON".to_owned()))?;
        if request.get("cmd").and_then(Value::as_str) == Some("begin")
            && value.get("ok") == Some(&Value::Bool(true))
            && serde_json::from_str::<BeginWire>(&response).is_err()
        {
            value["sync"] = Value::Null;
        }
        Ok(value)
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
    pin: Option<OraclePin>,
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
            pin: None,
            pool: Mutex::new(Pool::default()),
            freed: Condvar::new(),
            hello: Mutex::new(None),
        }
    }

    /// Refuses workers of another oracle than the pin
    /// (`conformance/oracle.json`): a run against them would compare goldens
    /// with a different Anki.
    pub fn expecting(mut self, pin: OraclePin) -> OracleExecutor {
        self.pin = Some(pin);
        self
    }

    /// The first hello a worker answered; every later worker matched it.
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

    /// Starts a worker and checks its hello against the pin and the first
    /// worker's: one run never mixes oracles.
    fn start(&self, tz: &str) -> Result<Worker, ExecError> {
        let mut worker = Worker::start(&self.command, tz)?;
        let response = worker.request(&json!({"cmd": "hello"}))?;
        let hello = Hello::from_response(&response)
            .filter(|_| response.get("ok") == Some(&Value::Bool(true)))
            .ok_or_else(|| ExecError::new("worker_start", "no hello from the worker"))?;
        if let Some(pin) = &self.pin
            && (hello.anki_version != pin.anki_version || hello.lock_sha256 != pin.lock_sha256)
        {
            return Err(ExecError::new(
                "oracle_mismatch",
                format!(
                    "the {tz} worker runs anki {} with lock {}, the pin is anki {} with lock {} (conformance/oracle.json)",
                    hello.anki_version,
                    short(&hello.lock_sha256),
                    pin.anki_version,
                    short(&pin.lock_sha256)
                ),
            ));
        }
        let mut first = self.hello.lock().unwrap_or_else(|e| e.into_inner());
        match &*first {
            Some(seen)
                if (&seen.anki_version, &seen.lock_sha256, &seen.driver_sha256)
                    != (&hello.anki_version, &hello.lock_sha256, &hello.driver_sha256) =>
            {
                Err(ExecError::new(
                    "oracle_mismatch",
                    format!(
                        "the {tz} worker runs anki {} (lock {}, driver {}), an earlier worker anki {} (lock {}, driver {})",
                        hello.anki_version,
                        short(&hello.lock_sha256),
                        short(&hello.driver_sha256),
                        seen.anki_version,
                        short(&seen.lock_sha256),
                        short(&seen.driver_sha256)
                    ),
                ))
            }
            Some(_) => Ok(worker),
            None => {
                *first = Some(hello);
                Ok(worker)
            }
        }
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

    /// One `run` request on a worker of the scenario's zone; the response's
    /// `observations.steps` and the whole response.
    fn run_request(&self, loaded: &LoadedScenario, request: Value) -> Result<(Vec<Value>, Value), ExecError> {
        let mut worker = self.checkout(&loaded.scenario.clock.tz)?;
        let response = match worker.request(&request) {
            Ok(response) => response,
            Err(e) => {
                drop(worker);
                self.release_slot();
                return Err(e);
            }
        };
        self.checkin(worker);
        if response.get("ok") != Some(&Value::Bool(true)) {
            return Err(protocol_error(response.get("error").and_then(Value::as_object)));
        }
        let steps = response
            .pointer("/observations/steps")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| ExecError::new("protocol_error", "a run response without observations"))?;
        Ok((steps, response))
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
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
        let request = json!({"cmd": "run", "id": loaded.scenario.id, "scenario": loaded.value});
        let (steps, _) = self.run_request(loaded, request)?;
        Ok(json!({"steps": steps}))
    }
}

impl SegmentExecutor for OracleExecutor {
    /// A `run` request for the segment: `resume`, `steps` and `handoff`
    /// (protocol.json); the worker reads and writes the handoff directory at
    /// its own mount.
    fn segment(&self, loaded: &LoadedScenario, request: &SegmentRequest) -> Result<SegmentOutput, ExecError> {
        let mut body = json!({
            "cmd": "run",
            "id": loaded.scenario.id,
            "scenario": loaded.value,
            "steps": [request.steps.start, request.steps.end],
        });
        if let Some(resume) = &request.resume {
            body["resume"] =
                serde_json::to_value(resume).map_err(|e| ExecError::new("internal_error", e.to_string()))?;
        }
        if let Some(dir) = &request.hand_off_to {
            body["handoff"] = json!(dir);
        }
        let (steps, response) = self.run_request(loaded, body)?;
        let handoff = match (&request.hand_off_to, response.get("state")) {
            (None, _) => None,
            (Some(_), Some(state)) => Some(
                serde_json::from_value::<Handoff>(state.clone())
                    .map_err(|e| ExecError::new("protocol_error", format!("handoff state: {e}")))?,
            ),
            (Some(_), None) => return Err(ExecError::new("protocol_error", "a handoff without a state")),
        };
        Ok(SegmentOutput { steps, handoff })
    }

    fn interactive(&self, tz: &str) -> Result<Box<dyn Interactive + '_>, ExecError> {
        let worker = self.checkout(tz)?;
        Ok(Box::new(Session {
            pool: self,
            worker: Some(worker),
            ended: false,
            failed: false,
        }))
    }
}

/// A worker held for one interactive session (protocol.json `begin`, `step`,
/// `end`). It goes back to the pool only after a successful `end`; otherwise
/// it is stopped, so no session stays open on a pooled worker.
struct Session<'a> {
    pool: &'a OracleExecutor,
    worker: Option<Worker>,
    ended: bool,
    failed: bool,
}

impl Interactive for Session<'_> {
    fn worker_identity(&self) -> Option<&str> {
        self.worker.as_ref().and_then(|w| w.identity.as_deref())
    }

    fn quarantine(&mut self) {
        self.failed = true;
    }

    fn request(&mut self, request: &Value) -> Result<Value, ExecError> {
        let worker = self
            .worker
            .as_mut()
            .ok_or_else(|| ExecError::new("worker_died", "the session's worker is gone"))?;
        self.ended = false;
        let response = match worker.request(request) {
            Ok(response) => response,
            Err(e) => {
                self.worker = None;
                self.pool.release_slot();
                return Err(e);
            }
        };
        if response.get("ok") != Some(&Value::Bool(true)) {
            self.failed = true;
            return Err(protocol_error(response.get("error").and_then(Value::as_object)));
        }
        self.ended = request.get("cmd").and_then(Value::as_str) == Some("end");
        Ok(response)
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let Some(worker) = self.worker.take() else { return };
        if self.ended && !self.failed {
            self.pool.checkin(worker);
        } else {
            drop(worker);
            self.pool.release_slot();
        }
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
