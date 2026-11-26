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

/// Implementation ceiling for editor URL bodies crossing the byte-based
/// bridge: 256 MiB each for encoded and expanded data. This is above P4's
/// 100 MiB warning so Add Anyway remains usable; it is not a sync file limit.
const MAX_EDITOR_IMAGE_BYTES: usize = 256 * 1024 * 1024;

pub fn fetch(threads: &ThreadRegistry, cancel: &CancelToken, written_url: &str) -> Result<FetchedMedia, SyncError> {
    let response = transport::runtime::block_on(threads, cancel,
        transport::http::get_editor_image(written_url, MAX_EDITOR_IMAGE_BYTES))?;
    let bytes = decode(response.bytes, &response.encoding, cancel, MAX_EDITOR_IMAGE_BYTES)?;
    Ok(FetchedMedia { name: fetched_name(written_url, &response.content_type), bytes })
}

fn decode(bytes: Vec<u8>, encoding: &str, cancel: &CancelToken, maximum_bytes: usize) -> Result<Vec<u8>, SyncError> {
    if cancel.is_cancelled() { return Err(SyncError::Cancelled); }
    if bytes.len() > maximum_bytes { return Err(size_failure()); }
    // Decoding is deliberately separate from sync's raw compressed transport.
    match encoding {
        "gzip" => read_bounded(flate2::read::MultiGzDecoder::new(bytes.as_slice()), cancel, maximum_bytes),
        "deflate" => read_bounded(flate2::read::ZlibDecoder::new(bytes.as_slice()), cancel, maximum_bytes),
        // Transfer ownership: identity must not need a second body allocation.
        "" | "identity" => Ok(bytes),
        _ => Err(failure("The response uses an unsupported content encoding.")),
    }
}

fn read_bounded(mut reader: impl Read, cancel: &CancelToken, maximum_bytes: usize) -> Result<Vec<u8>, SyncError> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        if cancel.is_cancelled() { return Err(SyncError::Cancelled); }
        // At the boundary read one extra byte to distinguish EOF from overflow,
        // without ever appending that byte or growing the destination for it.
        let available = buffer.len().min(maximum_bytes.saturating_sub(bytes.len()).saturating_add(1));
        let count = reader.read(&mut buffer[..available]).map_err(|e| failure(&e.to_string()))?;
        if count == 0 { break; }
        append_bounded(&mut bytes, &buffer[..count], maximum_bytes)?;
    }
    Ok(bytes)
}

/// Both encoded chunks and decoder output are admitted before allocating or
/// copying. Bound geometric capacity growth too, and report allocation failure.
pub(crate) fn append_bounded(bytes: &mut Vec<u8>, chunk: &[u8], maximum_bytes: usize) -> Result<(), SyncError> {
    let length = bytes.len().checked_add(chunk.len()).filter(|&n| n <= maximum_bytes)
        .ok_or_else(size_failure)?;
    if length > bytes.capacity() {
        let capacity = length.max(bytes.capacity().saturating_mul(2)).min(maximum_bytes);
        bytes.try_reserve_exact(capacity - bytes.len())
            .map_err(|_| failure("There is not enough memory for the editor image."))?;
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

pub(crate) fn size_failure() -> SyncError {
    failure("The editor image exceeds the in-memory safety limit.")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn chunks_are_checked_before_length_or_capacity_grows() {
        let mut bytes = Vec::new();
        append_bounded(&mut bytes, b"abcd", 8).unwrap();
        let capacity = bytes.capacity();
        assert_eq!(append_bounded(&mut bytes, b"efghi", 8), Err(size_failure()));
        assert_eq!(bytes, b"abcd");
        assert_eq!(bytes.capacity(), capacity);
        append_bounded(&mut bytes, b"efgh", 8).unwrap();
        assert_eq!(bytes, b"abcdefgh");
        assert_eq!(bytes.capacity(), 8);
        assert_eq!(append_bounded(&mut bytes, b"i", 8), Err(size_failure()));
        assert_eq!(bytes, b"abcdefgh");
        append_bounded(&mut Vec::new(), b"", 0).unwrap();
        assert_eq!(append_bounded(&mut Vec::new(), b"x", 0), Err(size_failure()));
    }

    #[test]
    fn identity_returns_the_original_allocation_and_checks_the_limit() {
        for encoding in ["", "identity"] {
            let bytes = b"abcd".to_vec();
            let pointer = bytes.as_ptr();
            let decoded = decode(bytes, encoding, &CancelToken::new(), 4).unwrap();
            assert_eq!(decoded, b"abcd");
            assert_eq!(decoded.as_ptr(), pointer);
            assert_eq!(decode(b"abcde".to_vec(), encoding, &CancelToken::new(), 4), Err(size_failure()));
        }
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn expanded_bytes_are_bounded_for_gzip_deflate_and_multiple_members() {
        let plain = [b'x'; 64];
        let mut deflate = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        deflate.write_all(&plain).unwrap();
        let mut members = gzip(&plain[..32]);
        members.extend(gzip(&plain[32..]));
        for (encoding, encoded) in [("gzip", gzip(&plain)), ("deflate", deflate.finish().unwrap()), ("gzip", members)] {
            assert!(encoded.len() < 63); // Refusal must be from expansion, not the encoded ceiling.
            assert_eq!(decode(encoded.clone(), encoding, &CancelToken::new(), 64).unwrap(), plain);
            assert_eq!(decode(encoded, encoding, &CancelToken::new(), 63), Err(size_failure()));
        }
    }

    #[test]
    fn invalid_encoding_corrupt_data_and_cancellation_remain_errors() {
        for encoding in ["gzip", "deflate", "br"] {
            assert!(decode(b"invalid".to_vec(), encoding, &CancelToken::new(), 64).is_err());
        }
        let mut truncated = gzip(b"image");
        truncated.pop();
        assert!(decode(truncated, "gzip", &CancelToken::new(), 64).is_err());
        let cancel = CancelToken::new();
        cancel.cancel();
        for encoding in ["", "identity", "gzip", "deflate"] {
            assert_eq!(decode(Vec::new(), encoding, &cancel, 64), Err(SyncError::Cancelled));
        }
    }

    #[test]
    fn cancellation_between_decoder_reads_stops_expansion() {
        struct CancellingReader<'a>(&'a CancelToken);
        impl Read for CancellingReader<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                assert!(!self.0.is_cancelled(), "must not read again after cancellation");
                buffer[0] = b'x';
                self.0.cancel();
                Ok(1)
            }
        }
        let cancel = CancelToken::new();
        assert_eq!(read_bounded(CancellingReader(&cancel), &cancel, 8), Err(SyncError::Cancelled));
    }
}
