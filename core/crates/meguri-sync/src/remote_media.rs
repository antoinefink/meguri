//! Explicit editor image selection (editor/paste P10.2/P10.3). No collection,
//! credentials, sync protocol or disk writes are involved in this service.
use std::io::Read;

use meguri_engine::{CancelToken, ThreadRegistry};

use crate::{SyncError, transport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedMedia {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub fn fetch(threads: &ThreadRegistry, cancel: &CancelToken, written_url: &str) -> Result<FetchedMedia, SyncError> {
    let response = transport::runtime::block_on(threads, cancel, transport::http::get_editor_image(written_url))?;
    // Decoding is deliberately separate from sync's raw compressed transport.
    let mut reader: Box<dyn Read + '_> = match response.encoding.as_str() {
        "gzip" => Box::new(flate2::read::MultiGzDecoder::new(response.bytes.as_slice())),
        "deflate" => Box::new(flate2::read::ZlibDecoder::new(response.bytes.as_slice())),
        "" | "identity" => Box::new(response.bytes.as_slice()),
        _ => return Err(failure("The response uses an unsupported content encoding.")),
    };
    let mut bytes = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        if cancel.is_cancelled() { return Err(SyncError::Cancelled); }
        let count = reader.read(&mut buffer).map_err(|e| failure(&e.to_string()))?;
        if count == 0 { break; }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(FetchedMedia { name: fetched_name(written_url, &response.content_type), bytes })
}

pub(crate) fn failure(detail: &str) -> SyncError {
    SyncError::Network(crate::NetworkError {
        cause: crate::NetworkCause::Connection,
        head_arrived: false,
        detail: detail.to_owned(),
    })
}

/// Use the original text, never the parsed URL or a redirect's location.
pub fn fetched_name(written_url: &str, content_type: &str) -> String {
    let path = written_url.split(['?', '#']).next().unwrap_or_default();
    let decoded = meguri_text::iri::decode_name(path);
    let tail = decoded.rsplit('/').next().unwrap_or_default();
    let mut name = if tail.chars().all(|c| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)) {
        "paste".to_owned()
    } else { tail.to_owned() };
    if !name.trim_start_matches('.').contains('.') {
        let kind = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
        name.push_str(match kind.as_str() {
            "audio/mpeg" => ".mp3", "audio/ogg" => ".oga", "audio/opus" => ".opus",
            "audio/wav" => ".wav", "audio/webm" => ".weba", "audio/aac" => ".aac",
            "image/jpeg" => ".jpg", "image/png" => ".png", "image/svg+xml" => ".svg",
            "image/webp" => ".webp", "image/avif" => ".avif", _ => "",
        });
    }
    name
}
