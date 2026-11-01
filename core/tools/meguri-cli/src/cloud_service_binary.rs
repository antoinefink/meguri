//! Versioned, opt-in synthetic IPC only; never used by Service or production sync.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{Read, Write};

pub const FLAG: &str = "--binary-assets-v1";
// CLI and Swift share these limits for fixture attachments only.
pub const BODY_BYTES: usize = 4 * 1_048_576;
pub const BODY_COUNT: usize = 16;
pub const TOTAL_BYTES: usize = 8 * 1_048_576;
const BAD: &str = "malformed-attachments";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub version: u8,
    pub lengths: Vec<usize>,
    pub message: Value,
}

// Only schema-defined Record/RecordInput Asset.bytes leaves can carry indices.
// '*' is one collection index or field name, never a recursive JSON escape.
fn asset_leaf(path: &[String]) -> bool {
    const RECORDS: &[&[&str]] = &[
        &["operation", "Modify", "changes", "*", "Save", "record"],
        &["result", "Ok", "Record"],
        &["result", "Ok", "Modified", "*", "result", "Ok", "Saved"],
        &["result", "Ok", "Modified", "*", "result", "Err", "server_record"],
        &["result", "Ok", "ZoneChanges", "changes", "*", "Saved"],
        &["result", "Err", "server_record"],
    ];
    RECORDS.iter().any(|prefix| {
        let suffix = ["fields", "*", "Asset", "bytes"];
        path.len() == prefix.len() + suffix.len()
            && prefix
                .iter()
                .chain(suffix.iter())
                .zip(path)
                .all(|(a, b)| *a == "*" || *a == b.as_str())
    })
}
fn walk(
    value: &mut Value,
    path: &mut Vec<String>,
    leaf: &mut impl FnMut(&mut Value) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    if asset_leaf(path) {
        return leaf(value);
    }
    match value {
        Value::Object(object) => {
            let mut probe = path.clone();
            probe.extend(["*", "Asset", "bytes"].map(String::from));
            if object.contains_key("attachment") && !asset_leaf(&probe) {
                return Err(BAD);
            }
            for (key, value) in object {
                path.push(key.clone());
                walk(value, path, leaf)?;
                path.pop();
            }
        }
        Value::Array(array) => {
            for (index, value) in array.iter_mut().enumerate() {
                path.push(index.to_string());
                walk(value, path, leaf)?;
                path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn bounds(lengths: &[usize]) -> Result<(), &'static str> {
    if lengths.len() > BODY_COUNT {
        return Err("attachment-limit");
    }
    let mut total = 0;
    for &length in lengths {
        if length > BODY_BYTES || length > TOTAL_BYTES - total {
            return Err("attachment-limit");
        }
        total += length;
    }
    Ok(())
}
impl Header {
    pub fn parse(frame: &[u8]) -> Result<Self, &'static str> {
        let header: Self = serde_json::from_slice(frame).map_err(|_| BAD)?;
        if header.version != 1 {
            return Err(BAD);
        }
        bounds(&header.lengths)?;
        // Validate every index, uniqueness and complete use before body IO/allocation.
        header.message_with(None)?;
        Ok(header)
    }
    pub fn message_with(&self, bodies: Option<&[Vec<u8>]>) -> Result<Value, &'static str> {
        if let Some(bodies) = bodies {
            if bodies.len() != self.lengths.len()
                || bodies
                    .iter()
                    .zip(&self.lengths)
                    .any(|(body, length)| body.len() != *length)
            {
                return Err(BAD);
            }
        }
        let mut message = self.message.clone();
        let mut used = vec![false; self.lengths.len()];
        walk(&mut message, &mut Vec::new(), &mut |value| {
            let object = value.as_object().ok_or(BAD)?;
            let index = object
                .get("attachment")
                .and_then(Value::as_u64)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or(BAD)?;
            if object.len() != 1 || index >= used.len() || used[index] {
                return Err(BAD);
            }
            used[index] = true;
            *value = match bodies {
                Some(bodies) => json!(bodies.get(index).ok_or(BAD)?),
                None => json!([]),
            };
            Ok(())
        })?;
        if used.iter().any(|used| !used) {
            return Err(BAD);
        }
        Ok(message)
    }
    pub fn read_bodies(&self, input: &mut impl Read) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut bodies = Vec::new();
        for &length in &self.lengths {
            let mut body = vec![0; length];
            input.read_exact(&mut body).map_err(|_| "truncated-attachment")?;
            bodies.push(body);
        }
        Ok(bodies)
    }
}
pub fn encode(mut message: Value) -> Result<(Vec<u8>, Vec<Vec<u8>>), &'static str> {
    let mut bodies = Vec::new();
    let mut lengths = Vec::new();
    walk(&mut message, &mut Vec::new(), &mut |value| {
        let array = value.as_array().ok_or(BAD)?;
        lengths.push(array.len());
        bounds(&lengths)?;
        let body: Vec<u8> = array
            .iter()
            .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()).ok_or(BAD))
            .collect::<Result<_, _>>()?;
        *value = json!({"attachment": bodies.len()});
        bodies.push(body);
        Ok(())
    })?;
    let header = serde_json::to_vec(&Header {
        version: 1,
        lengths,
        message,
    })
    .map_err(|_| "output")?;
    if header.len() > super::FRAME_BYTES {
        return Err("frame-too-large");
    }
    Ok((header, bodies))
}
pub fn write(message: Value, output: &mut impl Write) -> Result<(), &'static str> {
    let (header, bodies) = encode(message)?;
    output.write_all(&header).map_err(|_| "output")?;
    output.write_all(b"\n").map_err(|_| "output")?;
    for body in bodies {
        output.write_all(&body).map_err(|_| "output")?;
    }
    Ok(())
}
