//! What travels inside a request and a response (specs/sync/transport.md
//! T2, T5, T6): the `anki-sync` header, zstd request bodies compressed as they
//! are sent, the `anki-original-size` check and capped decompression of
//! successful bodies, and the reason text of a failed status.

use std::fmt::Write as _;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes};
use futures_core::Stream;
use meguri_engine::ProgressSink;
use zstd::stream::raw::{Decoder, Encoder, Operation, OutBuffer};

use crate::error::ProtocolError;
use crate::transport::RawStream;

/// The response header that declares the decompressed size (T6.2).
pub(crate) const ORIGINAL_SIZE: &str = "anki-original-size";

/// The zstd level of request bodies; any level works (T5).
const LEVEL: i32 = 3;

/// The raw bytes taken from the caller per compression step, and the most
/// decompressed bytes handed out at once.
const CHUNK: usize = 64 * 1024;

/// How much of a failed status's body becomes its reason (T6.1).
pub(crate) const REASON_LIMIT: usize = 16 * 1024;

/// The `anki-sync` header value (T2): compact JSON with `v`, `k`, `c` and `s`
/// in that order, printable ASCII only.
pub(crate) fn sync_header(host_key: &str, identity: &str, session_key: &str) -> String {
    let mut out = String::from("{\"v\":11,\"k\":");
    push_ascii_json_string(&mut out, host_key);
    out.push_str(",\"c\":");
    push_ascii_json_string(&mut out, identity);
    out.push_str(",\"s\":");
    push_ascii_json_string(&mut out, session_key);
    out.push('}');
    out
}

/// A JSON string whose every character outside U+0020..=U+007E is a `\uXXXX`
/// escape (surrogate pairs above U+FFFF): the server refuses raw non-ASCII
/// header bytes (T2).
fn push_ascii_json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    // Writing to a String cannot fail.
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

/// The declared decompressed size: exactly one header of 1 to 20 ASCII digits
/// that fits in a u64 (T6.2).
pub(crate) fn original_size(values: &[&[u8]]) -> Result<u64, ProtocolError> {
    let value = match values {
        [] => return Err(ProtocolError::MissingOriginalSize),
        [value] => *value,
        _ => return Err(ProtocolError::BadOriginalSize),
    };
    if value.is_empty() || value.len() > 20 || !value.iter().all(u8::is_ascii_digit) {
        return Err(ProtocolError::BadOriginalSize);
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|digits| digits.parse::<u64>().ok())
        .ok_or(ProtocolError::BadOriginalSize)
}

/// A failed status's reason: the body's first bytes as UTF-8, invalid bytes
/// replaced, surrounding white space trimmed (T6.1).
pub(crate) fn reason_text(body: &[u8]) -> String {
    let body = &body[..body.len().min(REASON_LIMIT)];
    String::from_utf8_lossy(body).trim().to_owned()
}

/// Where a request body's raw bytes come from.
pub(crate) enum Source {
    /// JSON held in memory, taken in [`CHUNK`]s.
    Bytes(Bytes),
    Stream(RawStream),
}

/// A request body, zstd-compressed while it is sent (T5): each raw chunk is
/// compressed and flushed at once, so a slow source still moves bytes, and
/// nothing is held twice. Counts the raw bytes the compressor took (T15).
pub(crate) struct CompressedBody {
    source: Source,
    encoder: Encoder<'static>,
    scratch: Vec<u8>,
    progress: Option<ProgressSink>,
    total: Option<u64>,
    taken: u64,
    done: bool,
}

impl CompressedBody {
    pub(crate) fn new(
        source: Source,
        total: Option<u64>,
        progress: Option<ProgressSink>,
    ) -> io::Result<CompressedBody> {
        Ok(CompressedBody {
            source,
            encoder: Encoder::new(LEVEL)?,
            scratch: vec![0; CHUNK],
            progress,
            total,
            taken: 0,
            done: false,
        })
    }

    /// Compresses one raw chunk and flushes the encoder.
    fn compress(&mut self, input: &[u8]) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut rest = input;
        while !rest.is_empty() {
            let status = self.encoder.run_on_buffers(rest, &mut self.scratch)?;
            out.extend_from_slice(&self.scratch[..status.bytes_written]);
            rest = &rest[status.bytes_read..];
        }
        loop {
            let (left, written) = {
                let mut output = OutBuffer::around(&mut self.scratch[..]);
                let left = self.encoder.flush(&mut output)?;
                (left, output.pos())
            };
            out.extend_from_slice(&self.scratch[..written]);
            if left == 0 {
                break;
            }
        }
        self.taken += input.len() as u64;
        if let Some(progress) = &self.progress {
            progress.report(self.taken, self.total);
        }
        Ok(out)
    }

    /// Ends the zstd frame.
    fn finish(&mut self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let (left, written) = {
                let mut output = OutBuffer::around(&mut self.scratch[..]);
                let left = self.encoder.finish(&mut output, false)?;
                (left, output.pos())
            };
            out.extend_from_slice(&self.scratch[..written]);
            if left == 0 {
                return Ok(out);
            }
        }
    }

    fn next_raw(&mut self, cx: &mut Context<'_>) -> Poll<Option<io::Result<Bytes>>> {
        match &mut self.source {
            Source::Bytes(bytes) if bytes.is_empty() => Poll::Ready(None),
            Source::Bytes(bytes) => Poll::Ready(Some(Ok(bytes.split_to(bytes.len().min(CHUNK))))),
            Source::Stream(stream) => stream.as_mut().poll_next(cx),
        }
    }
}

impl Stream for CompressedBody {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<io::Result<Bytes>>> {
        let this = &mut *self;
        loop {
            if this.done {
                return Poll::Ready(None);
            }
            let step = match this.next_raw(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => this.compress(&chunk),
                Poll::Ready(Some(Err(err))) => Err(err),
                Poll::Ready(None) => {
                    this.done = true;
                    this.finish()
                }
            };
            match step {
                Ok(out) if out.is_empty() => continue,
                Ok(out) => return Poll::Ready(Some(Ok(Bytes::from(out)))),
                Err(err) => {
                    this.done = true;
                    return Poll::Ready(Some(Err(err)));
                }
            }
        }
    }
}

/// Decompresses a successful body as it arrives and holds it to its declared
/// size (T6.2): one byte past it is `size_exceeded` and is never handed out,
/// an early end is `size_short`, and anything but complete zstd frames is
/// `bad_compression`. It decodes one piece of at most [`CHUNK`] bytes per
/// step, so memory grows only with what the caller takes, never with the
/// declared size or the compression ratio (R1852).
pub(crate) struct Decompressor {
    decoder: Decoder<'static>,
    declared: u64,
    produced: u64,
    received: bool,
    /// The last step ended a frame (and flushed it).
    at_frame_end: bool,
    /// The last step filled its output inside a frame: the decoder may hold
    /// more.
    pending_output: bool,
    scratch: Vec<u8>,
}

impl Decompressor {
    pub(crate) fn new(declared: u64) -> Result<Decompressor, ProtocolError> {
        Ok(Decompressor {
            decoder: Decoder::new().map_err(|_| ProtocolError::BadCompression)?,
            declared,
            produced: 0,
            received: false,
            at_frame_end: true,
            pending_output: false,
            scratch: vec![0; CHUNK],
        })
    }

    pub(crate) fn declared(&self) -> u64 {
        self.declared
    }

    pub(crate) fn produced(&self) -> u64 {
        self.produced
    }

    /// Decodes from the front of `input`, dropping what it consumed, until it
    /// has one piece of output. `None` when it needs more input: `input` is
    /// empty and the decoder holds nothing back.
    pub(crate) fn step(&mut self, input: &mut Bytes) -> Result<Option<Bytes>, ProtocolError> {
        loop {
            if input.is_empty() && !self.pending_output {
                return Ok(None);
            }
            if !input.is_empty() {
                self.received = true;
                if self.at_frame_end {
                    // A new frame starts (or garbage after the last one).
                    self.decoder.reinit().map_err(|_| ProtocolError::BadCompression)?;
                }
            }
            // Room for what the declared size still allows, plus one byte to
            // see an excess.
            let allowed = self.declared - self.produced;
            let room = usize::try_from(allowed.saturating_add(1)).map_or(CHUNK, |r| r.min(CHUNK));
            let status = self
                .decoder
                .run_on_buffers(input, &mut self.scratch[..room])
                .map_err(|_| ProtocolError::BadCompression)?;
            input.advance(status.bytes_read);
            if status.bytes_written as u64 > allowed {
                return Err(ProtocolError::SizeExceeded);
            }
            self.at_frame_end = status.remaining == 0;
            // A full output may hide more, unless the frame ended with it: a
            // step after the end would read as the start of a new frame.
            self.pending_output = status.bytes_written == room && !self.at_frame_end;
            if status.bytes_written > 0 {
                self.produced += status.bytes_written as u64;
                return Ok(Some(Bytes::copy_from_slice(&self.scratch[..status.bytes_written])));
            }
            if status.bytes_read == 0 {
                if !input.is_empty() {
                    // No progress with input left and room to write.
                    return Err(ProtocolError::BadCompression);
                }
                self.pending_output = false;
            }
        }
    }

    /// The body ended and every step returned `None`: it must have ended a
    /// frame and reached the declared size.
    pub(crate) fn finish(&self) -> Result<(), ProtocolError> {
        if self.received && !self.at_frame_end {
            return Err(ProtocolError::BadCompression);
        }
        if self.produced < self.declared {
            return Err(ProtocolError::SizeShort);
        }
        Ok(())
    }
}
