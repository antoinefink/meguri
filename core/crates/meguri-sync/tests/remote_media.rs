use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use meguri_engine::{CancelToken, ThreadRegistry};
use meguri_sync::{SyncError, remote_media::{fetch, fetched_name}};

const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\"><rect width=\"20\" height=\"10\"/></svg>";

fn response(status: u16, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 {status} Fixture\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn request(stream: &mut std::net::TcpStream) -> String {
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 16384);
    }
    String::from_utf8(bytes).unwrap()
}

/// Bound every accept; a missing request fails locally instead of leaving a
/// fixture thread behind. Only the loopback listener is ever reachable.
fn fixture(responses: Vec<Vec<u8>>, run: impl FnOnce(&str)) -> Vec<String> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let (stop, stopping) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for bytes in responses {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if stopping.recv_timeout(Duration::from_millis(2)).is_ok() { return requests; }
                    }
                    Err(e) => panic!("loopback accept: {e}"),
                }
            };
            requests.push(request(&mut stream));
            stream.write_all(&bytes).unwrap();
        }
        requests
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&url)));
    let _ = stop.send(());
    let requests = worker.join().unwrap();
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
    requests
}

#[test]
fn names_use_written_url_before_normalization_and_redirects() {
    for (url, kind, name) in [
        ("https://fixture.test/dir/a%2Fb.png?size=2#top", "image/jpeg", "b.png"),
        ("https://fixture.test/img?id=3", "IMAGE/PNG; charset=binary", "img.png"),
        ("https://fixture.test/", "image/jpeg; charset=binary", "paste.jpg"),
        ("https://fixture.test/.hidden", "image/png", ".hidden.png"),
        ("https://fixture.test/a+b.png#top", "image/png", "a+b.png"),
        ("https://fixture.test/a%2541.png", "image/png", "a%41.png"),
        ("https://fixture.test/%FF.png", "image/png", "\u{fffd}.png"),
        ("https://fixture.test/%20%1c", "image/svg+xml", "paste.svg"),
    ] { assert_eq!(fetched_name(url, kind), name); }
}

#[test]
fn gets_once_with_required_headers_and_keeps_bytes() {
    let requests = fixture(vec![response(200, "Content-Type: image/svg+xml\r\n", SVG)], |base| {
        let image = fetch(&ThreadRegistry::new(), &CancelToken::new(), &format!("{base}dir/../chosen.svg")).unwrap();
        assert_eq!(image.name, "chosen.svg");
        assert_eq!(image.bytes, SVG);
    });
    assert_eq!(requests.len(), 1);
    let request = requests[0].to_ascii_lowercase();
    assert!(request.starts_with("get /chosen.svg http/1.1\r\n"));
    assert!(request.contains("user-agent: mozilla/5.0 (compatible; meguri)\r\n"));
    assert!(request.contains("accept-encoding: gzip, deflate\r\n"));
    assert!(!request.contains("authorization:"));
}

#[test]
fn follows_each_supported_redirect_and_names_from_original() {
    let mut responses = [301, 302, 303, 307, 308].into_iter()
        .map(|status| response(status, "Location: /final\r\n", b"" )).collect::<Vec<_>>();
    responses.push(response(200, "Content-Type: image/svg+xml; charset=utf-8\r\n", SVG));
    let requests = fixture(responses, |base| {
        let image = fetch(&ThreadRegistry::new(), &CancelToken::new(), &format!("{base}original")).unwrap();
        assert_eq!(image.name, "original.svg");
        assert_eq!(image.bytes, SVG);
    });
    assert_eq!(requests.len(), 6);
}

#[test]
fn decodes_gzip_and_deflate() {
    for gzip in [true, false] {
        let (encoding, body) = if gzip {
            let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(SVG).unwrap();
            ("gzip", encoder.finish().unwrap())
        } else {
            let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(SVG).unwrap();
            ("deflate", encoder.finish().unwrap())
        };
        fixture(vec![response(200, &format!("Content-Type: image/svg+xml\r\nContent-Encoding: {encoding}\r\n"), &body)], |base| {
            let image = fetch(&ThreadRegistry::new(), &CancelToken::new(), base).unwrap();
            assert_eq!(image.bytes, SVG);
            assert_eq!(image.name, "paste.svg");
        });
    }
}

#[test]
fn refuses_bad_status_unsupported_scheme_and_eleventh_redirect() {
    for status in [204, 304, 404, 503] {
        fixture(vec![response(status, "", b"")], |base| {
            assert!(matches!(fetch(&ThreadRegistry::new(), &CancelToken::new(), base), Err(SyncError::Status { code, .. }) if code == status));
        });
    }
    fixture(vec![response(302, "Location: file:///never-read.svg\r\n", b"")], |base| {
        assert!(fetch(&ThreadRegistry::new(), &CancelToken::new(), base).is_err());
    });
    let requests = fixture(vec![response(302, "Location: /again\r\n", b""); 11], |base| {
        assert!(fetch(&ThreadRegistry::new(), &CancelToken::new(), base).is_err());
    });
    assert_eq!(requests.len(), 11);
    let mut ten = vec![response(302, "Location: /again\r\n", b""); 10];
    ten.push(response(200, "Content-Type: image/svg+xml\r\n", SVG));
    assert_eq!(fixture(ten, |base| {
        assert_eq!(fetch(&ThreadRegistry::new(), &CancelToken::new(), base).unwrap().bytes, SVG);
    }).len(), 11);
    for url in ["ftp://fixture.test/a.svg", "file:///never-read.svg"] {
        assert!(fetch(&ThreadRegistry::new(), &CancelToken::new(), url).is_err());
    }
}

#[test]
fn pre_cancelled_image_request_never_connects() {
    let cancel = CancelToken::new();
    cancel.cancel();
    fixture(Vec::new(), |base| {
        assert_eq!(fetch(&ThreadRegistry::new(), &cancel, base), Err(SyncError::Cancelled));
    });
}

#[test]
fn a_cancelled_token_editor_image_closes_held_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/slow.svg", listener.local_addr().unwrap());
    let (entered, admitted) = mpsc::channel();
    let (closed, disconnected) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nx").unwrap();
        entered.send(()).unwrap();
        let mut byte = [0];
        closed.send(stream.read(&mut byte).unwrap() == 0).unwrap();
    });
    let token = std::sync::Arc::new(CancelToken::new());
    let cancel = token.clone();
    let worker = std::thread::spawn(move || fetch(&ThreadRegistry::new(), &token, &url));
    admitted.recv_timeout(Duration::from_secs(5)).unwrap();
    cancel.cancel();
    assert_eq!(worker.join().unwrap(), Err(SyncError::Cancelled));
    assert!(disconnected.recv_timeout(Duration::from_secs(5)).unwrap());
    server.join().unwrap();
}
