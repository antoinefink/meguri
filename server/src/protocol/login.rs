// SPDX-License-Identifier: MIT
use serde::Deserialize;
#[derive(Deserialize)]
pub struct Header { pub v:i32, pub k:String, pub c:String, pub s:String }
#[derive(Deserialize)]
pub struct Login { pub u:String, pub p:String }
