//! The sync stub (T5.5): a plain HTTP/1.1 server on 127.0.0.1 that stands in
//! for a sync server for the full upload and download operations, and the
//! raw zstd frame it answers with (T5.5.1). It does no server-side work: it
//! reads each request body to its end, discards it and answers at once.
//!
//! Each answer is one write of a prepared buffer (headers and body
//! together) on a socket without Nagle's algorithm, so a client never waits
//! for a delayed acknowledgement between the two.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

/// T5.5.1: the size of every raw block but the last.
pub const PIECE: usize = 131_072;

/// T5.5.1: `content` in a zstd frame of raw blocks: the magic number, the
/// frame header (descriptor `00`, window descriptor `38`), then pieces of
/// [`PIECE`] bytes, each behind a 3-byte little-endian block header
/// `last + 8 × size`; no checksum.
pub fn raw_zstd_frame(content: &[u8]) -> Vec<u8> {
    let mut pieces: Vec<&[u8]> = content.chunks(PIECE).collect();
    if pieces.is_empty() {
        pieces.push(&[]);
    }
    let mut out = Vec::with_capacity(6 + 3 * pieces.len() + content.len());
    out.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x38]);
    let last = pieces.len() - 1;
    for (index, piece) in pieces.iter().enumerate() {
        // A piece is at most 131,072 bytes, so the header fits in 3 bytes.
        let header = u32::from(index == last) + 8 * piece.len() as u32;
        out.extend_from_slice(&header.to_le_bytes()[..3]);
        out.extend_from_slice(piece);
    }
    out
}

/// A 200 answer carrying `content` in the raw frame.
fn answer(content: &[u8]) -> Vec<u8> {
    let body = raw_zstd_frame(content);
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nanki-original-size: {}\r\nContent-Length: {}\r\n\r\n",
        content.len(),
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(&body);
    out
}

const NOT_FOUND: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";

/// The open connections: a handle to shut each down, and its thread.
type Connections = Arc<Mutex<Vec<(TcpStream, JoinHandle<()>)>>>;

/// The running stub. Dropping it stops it and closes its connections.
#[derive(Debug)]
pub struct SyncStub {
    port: u16,
    stopping: Arc<AtomicBool>,
    connections: Connections,
    acceptor: Option<JoinHandle<()>>,
}

struct Answers {
    upload: Vec<u8>,
    download: Vec<u8>,
}

impl SyncStub {
    /// Starts the stub on 127.0.0.1 at a free port, in its own threads.
    /// `/sync/download` answers with `download` (the image's bytes).
    pub fn start(download: &[u8]) -> io::Result<SyncStub> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let answers = Arc::new(Answers {
            upload: answer(b"OK"),
            download: answer(download),
        });
        let stopping = Arc::new(AtomicBool::new(false));
        let connections: Connections = Arc::default();
        let acceptor = {
            let stopping = Arc::clone(&stopping);
            let connections = Arc::clone(&connections);
            thread::Builder::new()
                .name("timing-sync-stub".to_owned())
                .spawn(move || {
                    for stream in listener.incoming() {
                        if stopping.load(Ordering::SeqCst) {
                            break;
                        }
                        let Ok(stream) = stream else { continue };
                        let Ok(handle) = stream.try_clone() else { continue };
                        let answers = Arc::clone(&answers);
                        let served = thread::Builder::new()
                            .name("timing-sync-stub-conn".to_owned())
                            .spawn(move || {
                                // A broken connection ends here; the client sees it.
                                let _ = serve(stream, &answers);
                            });
                        if let Ok(served) = served {
                            connections
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push((handle, served));
                        }
                    }
                })?
        };
        Ok(SyncStub {
            port,
            stopping,
            connections,
            acceptor: Some(acceptor),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The endpoint a sync client is given: `http://127.0.0.1:<port>/`.
    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }
}

impl Drop for SyncStub {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Wakes the acceptor, which then sees `stopping`.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
        let open = std::mem::take(&mut *self.connections.lock().unwrap_or_else(PoisonError::into_inner));
        for (stream, served) in open {
            let _ = stream.shutdown(Shutdown::Both);
            let _ = served.join();
        }
    }
}

/// Serves the requests of one connection until the client closes it or
/// asks to, then shuts the socket down (the stub keeps another handle to
/// it, so dropping this one would not close it).
fn serve(stream: TcpStream, answers: &Answers) -> io::Result<()> {
    let served = serve_requests(&stream, answers);
    let _ = stream.shutdown(Shutdown::Both);
    served
}

fn serve_requests(stream: &TcpStream, answers: &Answers) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let mut writer = stream;
    let mut reader = BufReader::new(stream);
    loop {
        let Some(request) = read_head(&mut reader)? else {
            return Ok(());
        };
        if request.chunked {
            read_chunked(&mut reader)?;
        } else if request.length > 0 {
            let skipped = io::copy(&mut (&mut reader).take(request.length), &mut io::sink())?;
            if skipped < request.length {
                return Ok(());
            }
        }
        let path = request.target.split('?').next().unwrap_or("");
        let reply: &[u8] = match (request.method.as_str(), path) {
            ("POST", "/sync/upload") => &answers.upload,
            ("POST", "/sync/download") => &answers.download,
            _ => NOT_FOUND,
        };
        writer.write_all(reply)?;
        writer.flush()?;
        if request.close {
            return Ok(());
        }
    }
}

struct Head {
    method: String,
    target: String,
    length: u64,
    chunked: bool,
    /// HTTP/1.0, or `Connection: close`.
    close: bool,
}

fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    if reader.read_until(b'\n', &mut line)? == 0 {
        return Ok(None);
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(Some(String::from_utf8_lossy(&line).into_owned()))
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_owned())
}

/// The request line and headers; `None` when the client closed the
/// connection between requests.
fn read_head(reader: &mut impl BufRead) -> io::Result<Option<Head>> {
    let Some(line) = read_line(reader)? else {
        return Ok(None);
    };
    let mut parts = line.split_ascii_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad("not an HTTP request line"));
    };
    let mut head = Head {
        method: method.to_owned(),
        target: target.to_owned(),
        length: 0,
        chunked: false,
        close: version == "HTTP/1.0",
    };
    loop {
        let line = read_line(reader)?.ok_or_else(|| bad("the headers end early"))?;
        if line.is_empty() {
            return Ok(Some(head));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(bad("a header without ':'"));
        };
        let value = value.trim().to_ascii_lowercase();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => head.length = value.parse().map_err(|_| bad("Content-Length"))?,
            "transfer-encoding" => head.chunked = value.split(',').any(|v| v.trim() == "chunked"),
            "connection" => head.close = value.split(',').any(|v| v.trim() == "close"),
            _ => {}
        }
    }
}

/// Reads and discards a chunked body, trailers included.
fn read_chunked(reader: &mut impl BufRead) -> io::Result<()> {
    loop {
        let line = read_line(reader)?.ok_or_else(|| bad("the body ends early"))?;
        let size = line.split(';').next().unwrap_or("").trim();
        let size = u64::from_str_radix(size, 16).map_err(|_| bad("a chunk size"))?;
        if size == 0 {
            while !read_line(reader)?
                .ok_or_else(|| bad("the trailers end early"))?
                .is_empty()
            {}
            return Ok(());
        }
        let skipped = io::copy(&mut (&mut *reader).take(size), &mut io::sink())?;
        if skipped < size {
            return Err(bad("the body ends early"));
        }
        read_line(reader)?.ok_or_else(|| bad("the body ends early"))?;
    }
}
