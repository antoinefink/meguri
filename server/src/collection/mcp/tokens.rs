// SPDX-License-Identifier: MIT
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    pub context: Context,
    pub tool: String,
    pub shape: String,
    pub kind: u8,
    pub after: i64,
}
impl Backend {
    // HMAC-SHA256 using the existing RustCrypto primitive. Fixed 32-byte key;
    // unlike a prefix hash this construction is not length-extension vulnerable.
    fn mac(&self, domain: &[u8], payload: &[u8]) -> [u8;32] {
        let mut inner = [0x36;64]; let mut outer = [0x5c;64];
        for i in 0..32 { inner[i] ^= self.key[i]; outer[i] ^= self.key[i]; }
        let mut hash = Sha256::new(); hash.update(inner); hash.update(domain); hash.update(payload);
        let sum = hash.finalize();
        let mut hash = Sha256::new(); hash.update(outer); hash.update(sum); hash.finalize().into()
    }
    pub(super) fn page_token(&self, context: &Context, tool: &str, shape: &str, kind: u8, after: i64) -> Result<String> {
        let payload = serde_json::to_vec(&Page { context: context.clone(), tool:tool.into(), shape:shape.into(), kind, after })
            .map_err(|_| unsupported())?;
        Ok(format!("{}.{}", URL_SAFE_NO_PAD.encode(&payload), URL_SAFE_NO_PAD.encode(self.mac(b"mcp-page-v1", &payload))))
    }
    pub(super) fn page(&self, token: Option<&str>, context: &Context, tool: &str, shape: &str) -> Result<Option<Page>> {
        let Some(token) = token else { return Ok(None); };
        if token.len() > TOKEN_RESERVE { return Err(invalid()); }
        let (body,signature) = token.split_once('.').ok_or_else(invalid)?;
        let payload = URL_SAFE_NO_PAD.decode(body).map_err(|_| invalid())?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| invalid())?;
        if !crate::crypto::same_digest(&self.mac(b"mcp-page-v1", &payload), &signature) { return Err(invalid()); }
        let page: Page = serde_json::from_slice(&payload).map_err(|_| invalid())?;
        if page.context != *context { return Err(conflict()); }
        if page.tool != tool || page.shape != shape || page.after < 0 || page.kind > 1 { return Err(invalid()); }
        Ok(Some(page))
    }
    pub(super) fn revision(&self, context: &Context, state: &[u8;32]) -> Result<String> {
        let mut payload = serde_json::to_vec(context).map_err(|_| unsupported())?;
        payload.extend_from_slice(state);
        Ok(URL_SAFE_NO_PAD.encode(self.mac(b"mcp-note-v1", &payload)))
    }
}
pub(super) fn shape(value: &impl Serialize) -> Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(crate::crypto::digest(&serde_json::to_vec(value).map_err(|_| invalid())?)))
}
/// Length-prefix every component and include its SQLite storage class.
pub(super) fn hash_value(hash: &mut Sha256, v: rusqlite::types::ValueRef<'_>) {
    use rusqlite::types::ValueRef::*;
    let (kind, bytes): (u8, Vec<u8>) = match v {
        Null => (0,vec![]), Integer(i) => (1,i.to_le_bytes().to_vec()),
        Real(f) => (2,f.to_bits().to_le_bytes().to_vec()), Text(t) => (3,t.to_vec()), Blob(b) => (4,b.to_vec()),
    };
    hash.update([kind]); hash.update((bytes.len() as u64).to_le_bytes()); hash.update(bytes);
}
