//! Internal synthetic-fixture transport, retaining one Service until stdin EOF.
use meguri_model::rng::SeededRandom;
use meguri_testkit::cloud::{Caller, Config, Operation, Scope, Service};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::Path;

#[path = "cloud_service_binary.rs"]
mod binary;

// JSON bytes before LF, including whitespace; unchanged in either transport.
// This fixture cap does not change Service or production sync budgets.
const FRAME_BYTES: usize = 1_048_576;

pub fn main(args: &[String]) -> i32 {
    let Some(seed) = args
        .get(1)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    else {
        eprintln!("cloud service startup-error: arguments");
        return 1;
    };
    let Ok(seed) = seed.parse::<u64>() else {
        eprintln!("cloud service startup-error: arguments");
        return 1;
    };
    let binary_assets = args.get(2).is_some_and(|arg| arg == binary::FLAG);
    if !(args.len() == 2 || (args.len() == 3 && binary_assets)) || args[0].is_empty() {
        eprintln!("cloud service startup-error: arguments");
        return 1;
    }
    let Ok(mut service) = Service::open(Path::new(&args[0]), Config::default(), SeededRandom::new(seed)) else {
        eprintln!("cloud service startup-error: storage");
        return 1;
    };
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    match serve(&mut service, &mut input, &mut output, binary_assets) {
        Ok(()) => 0,
        Err(kind) => {
            // Fixed classifications only: never format an input, path or serde/IO error.
            let _ = writeln!(output, "{{\"protocol_error\":\"{kind}\"}}");
            let _ = output.flush();
            eprintln!("cloud service protocol-error: {kind}");
            2
        }
    }
}

fn serve(
    service: &mut Service<SeededRandom>,
    input: &mut impl BufRead,
    output: &mut impl Write,
    binary_assets: bool,
) -> Result<(), &'static str> {
    while let Some(frame) = read_frame(input)? {
        let (scope, caller, operation) = if binary_assets {
            let header = binary::Header::parse(&frame)?;
            // Check the real request schema/allowlist before reading any body.
            request_value(header.message_with(None)?)?;
            let bodies = header.read_bodies(input)?;
            request_value(header.message_with(Some(&bodies))?)?
        } else {
            request(&frame)?
        };
        let response = service.request(&scope, caller, operation).map_err(|_| "storage")?;
        if binary_assets {
            // Extract only from this actual, single Service.request return value.
            binary::write(serde_json::to_value(&response).map_err(|_| "output")?, output)?;
        } else {
            serde_json::to_writer(&mut *output, &response).map_err(|_| "output")?;
            output.write_all(b"\n").map_err(|_| "output")?;
        }
        output.flush().map_err(|_| "output")?;
    }
    output.flush().map_err(|_| "output")
}

fn read_frame(input: &mut impl BufRead) -> Result<Option<Vec<u8>>, &'static str> {
    let mut frame = Vec::new();
    loop {
        let available = input.fill_buf().map_err(|_| "input")?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err("truncated-frame")
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.unwrap_or(available.len());
        // Check before extending, including for an unterminated adversarial line.
        if count > FRAME_BYTES - frame.len() {
            return Err("frame-too-large");
        }
        frame.extend_from_slice(&available[..count]);
        input.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

fn object_fields(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| allowed.contains(&key.as_str())))
}

fn request(frame: &[u8]) -> Result<(Scope, Caller, Operation), &'static str> {
    request_value(serde_json::from_slice(frame).map_err(|_| "malformed-frame")?)
}

fn request_value(mut value: Value) -> Result<(Scope, Caller, Operation), &'static str> {
    if !object_fields(&value, &["scope", "caller", "operation"])
        || !object_fields(&value["scope"], &["container", "environment", "account"])
        || !object_fields(&value["caller"], &["device", "instance", "own_operation"])
    {
        return Err("malformed-frame");
    }
    let scope: Scope = serde_json::from_value(value["scope"].take()).map_err(|_| "malformed-frame")?;
    let caller: Caller = serde_json::from_value(value["caller"].take()).map_err(|_| "malformed-frame")?;
    let operation: Operation = serde_json::from_value(value["operation"].take()).map_err(|_| "malformed-frame")?;
    match operation {
        Operation::DeleteZone {
            cause: meguri_testkit::cloud::RemovalCause::Deleted,
            ..
        }
        | Operation::CreateZone { .. }
        | Operation::Modify { .. }
        | Operation::Read { .. }
        | Operation::FetchAllZones
        | Operation::FetchDatabase { .. }
        | Operation::FetchZone { .. } => Ok((scope, caller, operation)),
        _ => Err("unsupported-operation"),
    }
}
