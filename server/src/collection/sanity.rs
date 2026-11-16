// SPDX-License-Identifier: MIT
use crate::{Error, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct Counts(pub [u32;3],pub u32,pub u32,pub u32,pub u32,pub u32,pub u32,pub u32);
impl Counts { pub fn compared(mut self)->Self { self.0=[0;3];self.4=0;self } }
pub(crate) fn counts(c:&Connection)->Result<Counts> {
    let mut values=[0;6];
    for (n,table) in ["cards","notes","revlog","notetypes","decks","deck_config"].iter().enumerate() {
        let v:i64=c.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get(0))?;
        values[n]=u32::try_from(v).map_err(|_|Error::new("storage_unavailable"))?;
    }
    Ok(Counts([0;3],values[0],values[1],values[2],0,values[3],values[4],values[5]))
}
