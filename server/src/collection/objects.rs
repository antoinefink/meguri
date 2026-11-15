// SPDX-License-Identifier: MIT
//! SQL-to-public-model seam. No scheduler defaults or stock content enter it.
use crate::{Result, collection::{model_adapter::blob, schema::invalid}};
use meguri_model::{
    deck::Deck, deck_config::DeckConfig,
    notetype::{Notetype, NotetypeField, NotetypeTemplate},
    proto::{BlobMessage, notetype::{FieldConfig, TemplateConfig}},
    time::EpochSecs, usn::Usn,
};
use rusqlite::{Connection, Row};

fn row_blob<M: BlobMessage>(row: &Row<'_>, index: usize) -> Result<meguri_model::proto::ProtoBlob<M>> {
    blob(row.get_ref(index)?.as_blob().map_err(|_| invalid())?)
}
pub fn deck(row: &Row<'_>) -> Result<Deck> {
    Ok(Deck { id: row.get::<_,i64>(0)?.into(), name: row.get(1)?, mtime: EpochSecs(row.get(2)?), usn: Usn(row.get(3)?), common: row_blob(row,4)?, kind: row_blob(row,5)? })
}
pub fn preset(row: &Row<'_>) -> Result<DeckConfig> {
    Ok(DeckConfig { id: row.get::<_,i64>(0)?.into(), name: row.get(1)?, mtime: EpochSecs(row.get(2)?), usn: Usn(row.get(3)?), config: row_blob(row,4)? })
}
pub fn notetype(c: &Connection, row: &Row<'_>) -> Result<Notetype> {
    let id: i64 = row.get(0)?;
    let mut fields = Vec::new();
    let mut q = c.prepare("SELECT ord,name,config FROM fields WHERE ntid=?1 ORDER BY ord")?;
    let mut rows = q.query([id])?;
    while let Some(r) = rows.next()? {
        let ord: u32 = r.get(0)?;
        if ord as usize != fields.len() { return Err(invalid()); }
        fields.push(NotetypeField { ord, name:r.get(1)?,config:row_blob::<FieldConfig>(r,2)?,prior_ord:None });
    }
    let mut templates = Vec::new();
    let mut q = c.prepare("SELECT ord,name,mtime_secs,usn,config FROM templates WHERE ntid=?1 ORDER BY ord")?;
    let mut rows = q.query([id])?;
    while let Some(r) = rows.next()? {
        let ord: u32 = r.get(0)?;
        if ord as usize != templates.len() { return Err(invalid()); }
        templates.push(NotetypeTemplate { ord,name:r.get(1)?,mtime:EpochSecs(r.get(2)?),usn:Usn(r.get(3)?),config:row_blob::<TemplateConfig>(r,4)?,prior_ord:None });
    }
    Ok(Notetype { id:id.into(), name:row.get(1)?,mtime:EpochSecs(row.get(2)?),usn:Usn(row.get(3)?),config:row_blob(row,4)?,fields,templates })
}
pub(crate) fn validate(c: &Connection) -> Result<()> {
    let mut q = c.prepare("SELECT id,name,mtime_secs,usn,common,kind FROM decks")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? { deck(r)?; }
    let mut q = c.prepare("SELECT id,name,mtime_secs,usn,config FROM deck_config")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? { preset(r)?; }
    let mut q = c.prepare("SELECT id,name,mtime_secs,usn,config FROM notetypes")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? { notetype(c,r)?; }
    // Orphan field/template rows still have to be decodable; never drop them.
    for (table, template) in [("fields",false),("templates",true)] {
        let mut q = c.prepare(&format!("SELECT config FROM {table}"))?;
        let mut rows = q.query([])?;
        while let Some(r) = rows.next()? {
            if template { row_blob::<TemplateConfig>(r,0)?; } else { row_blob::<FieldConfig>(r,0)?; }
        }
    }
    Ok(())
}
