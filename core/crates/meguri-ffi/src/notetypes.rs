//! Narrow U29 operations. No structural save/draft API crosses FFI.
use crate::{CoreError, callback, mutations::MutationSummary, runtime};
use meguri_engine::notetypes::{self, AggregateDigest, Draft, SaveOptions};
use meguri_model::ids::NotetypeId;
use meguri_storage::{ReadRows, ReaderRole};
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum StockNotetype {
    Basic,
    BasicAndReversed,
    BasicOptionalReversed,
    BasicTyping,
    Cloze,
    ImageOcclusion,
}
impl From<StockNotetype> for notetypes::StockKind {
    fn from(k: StockNotetype) -> Self {
        match k {
            StockNotetype::Basic => Self::Basic,
            StockNotetype::BasicAndReversed => Self::BasicAndReversed,
            StockNotetype::BasicOptionalReversed => Self::BasicOptionalReversed,
            StockNotetype::BasicTyping => Self::BasicTyping,
            StockNotetype::Cloze => Self::Cloze,
            StockNotetype::ImageOcclusion => Self::ImageOcclusion,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct NewNotetypeTemplate {
    pub name: String,
    pub front: String,
    pub back: String,
    pub browser_front: String,
    pub browser_back: String,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct AddNotetypeRequest {
    pub name: String,
    pub cloze: bool,
    pub fields: Vec<String>,
    pub templates: Vec<NewNotetypeTemplate>,
    pub css: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeMutation {
    pub summary: MutationSummary,
    pub id: i64,
    pub name: String,
    pub written: bool,
}
impl From<meguri_engine::OpResult<notetypes::Added>> for NotetypeMutation {
    fn from(r: meguri_engine::OpResult<notetypes::Added>) -> Self {
        Self {
            summary: MutationSummary::of(&r),
            id: r.value.id.0,
            name: r.value.name,
            written: r.value.written,
        }
    }
}
impl From<meguri_engine::OpResult<notetypes::Saved>> for NotetypeMutation {
    fn from(r: meguri_engine::OpResult<notetypes::Saved>) -> Self {
        Self {
            summary: MutationSummary::of(&r),
            id: r.value.id.0,
            name: r.value.name,
            written: r.value.written,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeField {
    pub ord: u32,
    pub id: Option<i64>,
    pub name: String,
    pub sticky: bool,
    pub rtl: bool,
    pub font_name: String,
    pub font_size: u32,
    pub description: String,
    pub plain_text: bool,
    pub collapsed: bool,
    pub exclude_from_search: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeTemplate {
    pub ord: u32,
    pub id: Option<i64>,
    pub name: String,
    pub front: String,
    pub back: String,
    pub q_format_browser: String,
    pub a_format_browser: String,
    pub target_deck_id: i64,
    pub browser_font_name: String,
    pub browser_font_size: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeRecord {
    pub id: i64,
    pub name: String,
    pub kind: i32,
    pub sort_field: u32,
    pub css: String,
    pub base: Vec<u8>,
    pub fields: Vec<NotetypeField>,
    pub templates: Vec<NotetypeTemplate>,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NotetypeName {
    pub id: i64,
    pub name: String,
}
#[uniffi::export]
pub fn notetype_names() -> Result<Vec<NotetypeName>, CoreError> {
    callback::enter("notetype_names");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    Ok(notetypes::read::names(&*r)?
        .into_iter()
        .map(|(id, name)| NotetypeName { id: id.0, name })
        .collect())
}
#[uniffi::export]
pub fn read_notetype(id: i64) -> Result<NotetypeRecord, CoreError> {
    callback::enter("read_notetype");
    let c = runtime::host()?.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    let snapshot = r.snapshot()?;
    let n = snapshot
        .notetype(NotetypeId(id))?
        .ok_or_else(|| CoreError::InvalidArgument {
            message: "unknown note type".into(),
        })?;
    Ok(NotetypeRecord {
        id,
        name: n.name.clone(),
        kind: n.config.kind,
        sort_field: n.config.sort_field_idx,
        css: n.config.css.clone(),
        base: AggregateDigest::of(&n).0.to_vec(),
        fields: n
            .fields
            .iter()
            .map(|f| NotetypeField {
                ord: f.ord,
                id: f.config.id,
                name: f.name.clone(),
                sticky: f.config.sticky,
                rtl: f.config.rtl,
                font_name: f.config.font_name.clone(),
                font_size: f.config.font_size,
                description: f.config.description.clone(),
                plain_text: f.config.plain_text,
                collapsed: f.config.collapsed,
                exclude_from_search: f.config.exclude_from_search,
            })
            .collect(),
        templates: n
            .templates
            .iter()
            .map(|t| NotetypeTemplate {
                ord: t.ord,
                id: t.config.id,
                name: t.name.clone(),
                front: t.config.q_format.clone(),
                back: t.config.a_format.clone(),
                q_format_browser: t.config.q_format_browser.clone(),
                a_format_browser: t.config.a_format_browser.clone(),
                target_deck_id: t.config.target_deck_id,
                browser_font_name: t.config.browser_font_name.clone(),
                browser_font_size: t.config.browser_font_size,
            })
            .collect(),
    })
}
#[uniffi::export]
pub fn add_notetype(request: AddNotetypeRequest) -> Result<NotetypeMutation, CoreError> {
    callback::enter("add_notetype");
    let new = notetypes::NewNotetype {
        name: request.name,
        kind: if request.cloze {
            notetypes::NotetypeKind::Cloze
        } else {
            notetypes::NotetypeKind::Standard
        },
        fields: request.fields,
        css: request.css,
        templates: request
            .templates
            .into_iter()
            .map(|t| notetypes::NewTemplate {
                name: t.name,
                front: t.front,
                back: t.back,
                browser_front: t.browser_front,
                browser_back: t.browser_back,
            })
            .collect(),
    };
    Ok(notetypes::add(&runtime::host()?.collection()?, new)?.into())
}
#[uniffi::export]
pub fn add_stock_notetype(kind: StockNotetype, name: Option<String>) -> Result<NotetypeMutation, CoreError> {
    callback::enter("add_stock_notetype");
    Ok(notetypes::add_stock(&runtime::host()?.collection()?, kind.into(), name)?.into())
}
#[uniffi::export]
pub fn clone_notetype(id: i64, name: Option<String>) -> Result<NotetypeMutation, CoreError> {
    callback::enter("clone_notetype");
    Ok(notetypes::clone_notetype(&runtime::host()?.collection()?, NotetypeId(id), name)?.into())
}
#[uniffi::export]
pub fn ensure_image_occlusion_notetype() -> Result<NotetypeMutation, CoreError> {
    callback::enter("ensure_image_occlusion_notetype");
    Ok(notetypes::ensure_image_occlusion(&runtime::host()?.collection()?)?.into())
}
// Read intact records; only replace the explicitly requested member. save_in
// compares the caller's aggregate base again on the writer before any write.
fn draft(c: &meguri_engine::Collection, id: i64, base: Vec<u8>) -> Result<Draft, CoreError> {
    let base: [u8; 32] = base.try_into().map_err(|_| CoreError::InvalidArgument {
        message: "aggregate digest must be 32 bytes".into(),
    })?;
    let r = c.reader(ReaderRole::Query)?;
    let snapshot = r.snapshot()?;
    let n = snapshot
        .notetype(NotetypeId(id))?
        .ok_or_else(|| CoreError::InvalidArgument {
            message: "unknown note type".into(),
        })?;
    if AggregateDigest::of(&n).0 != base {
        return Err(meguri_engine::EngineError::Stale {
            table: meguri_storage::Table::Notetypes,
            key: id.to_string(),
        }
        .into());
    }
    Ok(Draft {
        notetype: n,
        base: AggregateDigest(base),
    })
}
#[uniffi::export]
pub fn rename_notetype(id: i64, base: Vec<u8>, name: String) -> Result<NotetypeMutation, CoreError> {
    callback::enter("rename_notetype");
    let c = runtime::host()?.collection()?;
    let mut d = draft(&c, id, base)?;
    d.name = name;
    Ok(notetypes::save(&c, d, SaveOptions::default())?.into())
}
#[uniffi::export]
pub fn set_notetype_css(id: i64, base: Vec<u8>, css: String) -> Result<NotetypeMutation, CoreError> {
    callback::enter("set_notetype_css");
    let c = runtime::host()?.collection()?;
    let mut d = draft(&c, id, base)?;
    d.config.css = css;
    Ok(notetypes::save(&c, d, SaveOptions::default())?.into())
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeFieldOptions {
    pub notetype_id: i64,
    pub base: Vec<u8>,
    pub ord: u32,
    pub id: Option<i64>,
    pub sticky: bool,
    pub rtl: bool,
    pub font_name: String,
    pub font_size: u32,
    pub description: String,
    pub plain_text: bool,
    pub collapsed: bool,
    pub exclude_from_search: bool,
}
#[uniffi::export]
pub fn set_notetype_field_options(request: NotetypeFieldOptions) -> Result<NotetypeMutation, CoreError> {
    callback::enter("set_notetype_field_options");
    let c = runtime::host()?.collection()?;
    let mut d = draft(&c, request.notetype_id, request.base)?;
    let row = d
        .fields
        .iter_mut()
        .find(|f| f.ord == request.ord && f.config.id == request.id)
        .ok_or_else(|| CoreError::InvalidArgument {
            message: "field identity no longer matches".into(),
        })?;
    row.config.sticky = request.sticky;
    row.config.rtl = request.rtl;
    row.config.font_name = request.font_name;
    row.config.font_size = request.font_size;
    row.config.description = request.description;
    row.config.plain_text = request.plain_text;
    row.config.collapsed = request.collapsed;
    row.config.exclude_from_search = request.exclude_from_search;
    Ok(notetypes::save(&c, d, SaveOptions::default())?.into())
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct NotetypeTemplateOptions {
    pub notetype_id: i64,
    pub base: Vec<u8>,
    pub ord: u32,
    pub id: Option<i64>,
    pub q_format_browser: String,
    pub a_format_browser: String,
    pub target_deck_id: i64,
    pub browser_font_name: String,
    pub browser_font_size: u32,
}
#[uniffi::export]
pub fn set_notetype_template_options(request: NotetypeTemplateOptions) -> Result<NotetypeMutation, CoreError> {
    callback::enter("set_notetype_template_options");
    let c = runtime::host()?.collection()?;
    let mut d = draft(&c, request.notetype_id, request.base)?;
    let row = d
        .templates
        .iter_mut()
        .find(|f| f.ord == request.ord && f.config.id == request.id)
        .ok_or_else(|| CoreError::InvalidArgument {
            message: "template identity no longer matches".into(),
        })?;
    row.config.q_format_browser = request.q_format_browser;
    row.config.a_format_browser = request.a_format_browser;
    row.config.target_deck_id = request.target_deck_id;
    row.config.browser_font_name = request.browser_font_name;
    row.config.browser_font_size = request.browser_font_size;
    Ok(notetypes::save(&c, d, SaveOptions::default())?.into())
}
