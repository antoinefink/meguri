// SPDX-License-Identifier: MIT
//! A bounded duplicate-preserving preflight, not a JSON-RPC dispatcher.
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::{fmt, io};

const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 16;

struct Seed<'a> { remaining: &'a mut usize, depth: usize }
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        if *self.remaining == 0 || self.depth > MAX_DEPTH {
            return Err(de::Error::custom("request structure limit"));
        }
        *self.remaining -= 1;
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("bounded JSON") }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value,E> { Ok(Value::Bool(v)) }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value,E> { Ok(v.into()) }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value,E> { Ok(v.into()) }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value,E> {
        serde_json::Number::from_f64(v).map(Value::Number).ok_or_else(|| E::custom("number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value,E> { Ok(v.into()) }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value,E> { Ok(Value::String(v)) }
    fn visit_unit<E: de::Error>(self) -> Result<Value,E> { Ok(Value::Null) }
    fn visit_none<E: de::Error>(self) -> Result<Value,E> { Ok(Value::Null) }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Value,S::Error> {
        let mut values = Vec::new();
        while let Some(v) = seq.next_element_seed(Seed { remaining:self.remaining, depth:self.depth+1 })? {
            values.push(v);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Value,M::Error> {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) { return Err(de::Error::custom("duplicate member")); }
            let value = map.next_value_seed(Seed { remaining:self.remaining, depth:self.depth+1 })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}
pub(super) fn parse(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    let mut remaining = MAX_NODES;
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let value = Seed { remaining:&mut remaining, depth:0 }.deserialize(&mut d)?;
    d.end()?;
    Ok(value)
}

struct Limited { bytes: Vec<u8>, max: usize }
impl io::Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.max - self.bytes.len() { return Err(io::Error::other("result limit")); }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
pub(super) fn encode(value: &impl serde::Serialize, max: usize) -> Result<Vec<u8>, ()> {
    let mut out = Limited { bytes:Vec::new(), max };
    serde_json::to_writer(&mut out, value).map_err(|_| ())?;
    Ok(out.bytes)
}
