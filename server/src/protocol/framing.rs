// SPDX-License-Identifier: MIT
//! Raw zstd, bounded channels, byte limits and progress deadlines. HTTP framing
//! remains Hyper's responsibility; no Content-Encoding layer is introduced.
use crate::{Error, Result, Service};
use axum::{body::{Body, Bytes}, response::{IntoResponse, Response}};
use http_body::{Body as HttpBody, Frame};
use serde::de::DeserializeOwned;
use std::{future::poll_fn, io::{self, Read, Write}, pin::Pin, sync::Arc, task::{Context,Poll}, time::Duration};
use tokio::sync::{mpsc, watch, Semaphore, OwnedSemaphorePermit};

const BLOCK: usize = 64 * 1024;
const SLOTS: u32 = 16;
const STALL: Duration = Duration::from_secs(60);
pub(crate) struct Ingress { slots: Arc<Semaphore>, encoders:Arc<Semaphore>, stopping: watch::Sender<bool> }
impl Default for Ingress {
    fn default() -> Self { Self { slots:Arc::new(Semaphore::new(SLOTS as usize)), encoders:Arc::new(Semaphore::new(SLOTS as usize)), stopping:watch::channel(false).0 } }
}
impl Ingress {
    pub fn enter(&self) -> Result<OwnedSemaphorePermit> {
        if *self.stopping.borrow() { return Err(Error::new("admission_closed")); }
        self.slots.clone().try_acquire_owned().map_err(|_|Error::limited(1))
    }
    pub fn close(&self) { self.stopping.send_replace(true); }
    pub async fn idle(&self) {
        let _all = self.slots.clone().acquire_many_owned(SLOTS).await;
        let _encoders=self.encoders.clone().acquire_many_owned(SLOTS).await;
    }
}

/// Typed deserialization uses original tokens, so i64 never visits binary64
/// and duplicate known members stay visible to serde's struct reader.
pub fn json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::new("invalid_request"))?;
    if bytes.iter().find(|b|!b.is_ascii_whitespace())!=Some(&b'{') { return Err(Error::new("invalid_request")); }
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for byte in bytes {
        if quoted {
            if escaped { escaped=false; } else if *byte == b'\\' { escaped=true; } else if *byte == b'"' { quoted=false; }
        } else {
            match *byte {
                b'"' => quoted=true,
                b'[' | b'{' => { depth+=1; if depth>127 { return Err(Error::new("invalid_request")); } },
                b']' | b'}' => { depth=depth.checked_sub(1).ok_or(Error::new("invalid_request"))?; },
                _ => (),
            }
        }
    }
    meguri_model::json_text::parse(text).ok_or(Error::new("invalid_request"))?;
    serde_json::from_slice(bytes).map_err(|_|Error::new("invalid_request"))
}

/// Stream decoded bytes into a sink. Allocation is independent of any declared
/// zstd content size, and the decoder's window has a separate memory bound.
pub fn decode<R: Read, W: Write>(reader: R, sink: &mut W, limit: u64) -> Result<()> {
    let mut decoder = zstd::stream::read::Decoder::new(reader).map_err(|_| Error::new("invalid_request"))?;
    decoder.window_log_max(27).map_err(|_| Error::new("invalid_request"))?;
    let mut block = [0;BLOCK];
    let mut total = 0u64;
    loop {
        // At most the first excess byte is decoded; never write it to the sink.
        let max = ((limit-total).min(BLOCK as u64 - 1) + 1) as usize;
        let count = decoder.read(&mut block[..max]).map_err(|_|Error::new("invalid_request"))?;
        if count == 0 { break; }
        if count as u64 > limit-total { return Err(Error::new("request_too_large")); }
        sink.write_all(&block[..count])?;
        total += count as u64;
    }
    Ok(())
}

enum Input { Bytes(Bytes), End }
struct Reader { rx:mpsc::Receiver<Input>, current:io::Cursor<Bytes>, ended:bool }
impl Read for Reader {
    fn read(&mut self, out:&mut [u8]) -> io::Result<usize> {
        loop {
            let n=self.current.read(out)?;
            if n!=0 || self.ended || out.is_empty() { return Ok(n); }
            match self.rx.blocking_recv() {
                Some(Input::Bytes(bytes)) => self.current=io::Cursor::new(bytes),
                Some(Input::End) => self.ended=true,
                None => return Err(io::Error::new(io::ErrorKind::UnexpectedEof,"incomplete native body")),
            }
        }
    }
}
pub(crate) async fn receive<T:Send+'static>(
    service:Arc<Service>, body:&mut Body,
    consume:impl FnOnce(Box<dyn Read+Send>,u64)->Result<T>+Send+'static,
) -> Result<T> {
    let (tx,rx)=mpsc::channel(2);
    let limit=service.config.native_decompressed_bytes.get();
    let owner=service.clone();
    let worker=tokio::task::spawn_blocking(move || {
        let _owner=owner;
        consume(Box::new(Reader { rx,current:io::Cursor::new(Bytes::new()),ended:false }),limit)
    });
    let mut stopping=service.native_ingress.stopping.subscribe();
    let feed=async {
        let mut compressed=0u64;
        let mut deadline=tokio::time::Instant::now()+STALL;
        loop {
            if *stopping.borrow_and_update() { return Err(Error::new("admission_closed")); }
            let next=tokio::select! {
                _=stopping.changed()=>return Err(Error::new("admission_closed")),
                result=tokio::time::timeout_at(deadline,poll_fn(|cx|Pin::new(&mut *body).poll_frame(cx)))=>result.map_err(|_|Error::new("invalid_request"))?,
            };
            let Some(frame)=next else { break; };
            if let Ok(bytes)=frame.map_err(|_|Error::new("invalid_request"))?.into_data() {
                if !bytes.is_empty() { deadline=tokio::time::Instant::now()+STALL; }
                compressed=compressed.checked_add(bytes.len() as u64).filter(|n|*n<=service.config.native_compressed_bytes.get()).ok_or(Error::new("request_too_large"))?;
                for part in bytes.chunks(BLOCK) {
                    tokio::time::timeout(STALL,tx.send(Input::Bytes(Bytes::copy_from_slice(part)))).await
                        .map_err(|_|Error::new("invalid_request"))?.map_err(|_|Error::new("invalid_request"))?;
                }
            }
            tokio::task::yield_now().await;
        }
        if compressed==0 { return Err(Error::new("invalid_request")); }
        tx.send(Input::End).await.map_err(|_|Error::new("invalid_request"))
    }.await;
    drop(tx);
    // Always join: a failed body cannot release root/budget while decoder writes.
    let result=worker.await.map_err(|_|Error::new("storage_unavailable"))?;
    match (feed,result) {
        (Err(e),Err(worker)) if e.code=="invalid_request" => Err(worker),
        (Err(e),_) => Err(e),
        (Ok(()),result) => result,
    }
}

pub(crate) fn response(bytes:&[u8]) -> Result<Response> {
    let compressed=zstd::stream::encode_all(bytes,0)?;
    Ok(([("anki-original-size",bytes.len().to_string())],compressed).into_response())
}

struct Writer { tx:mpsc::Sender<io::Result<Bytes>>, runtime:tokio::runtime::Handle }
impl Write for Writer {
    fn write(&mut self, bytes:&[u8])->io::Result<usize> {
        for part in bytes.chunks(BLOCK) {
            self.runtime.block_on(async {
                tokio::time::timeout(STALL,self.tx.send(Ok(Bytes::copy_from_slice(part)))).await
                    .map_err(|_|io::Error::new(io::ErrorKind::TimedOut,"native response stalled"))?
                    .map_err(|_|io::Error::new(io::ErrorKind::BrokenPipe,"native response closed"))
            })?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self)->io::Result<()> { Ok(()) }
}
struct Stream(mpsc::Receiver<io::Result<Bytes>>);
impl HttpBody for Stream {
    type Data=Bytes;
    type Error=io::Error;
    fn poll_frame(mut self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<Option<io::Result<Frame<Bytes>>>> {
        self.0.poll_recv(cx).map(|item|item.map(|r|r.map(Frame::data)))
    }
}
pub(crate) fn download(service:Arc<Service>, snapshot:super::full::Snapshot) -> Result<Response> {
    let size=snapshot.file.metadata()?.len();
    let (tx,rx)=mpsc::channel(2);
    let runtime=tokio::runtime::Handle::current();
    // Independent slot survives response cancellation until the encoder stops.
    let permit=service.native_ingress.encoders.clone().try_acquire_owned().map_err(|_|Error::limited(1))?;
    tokio::task::spawn_blocking(move || {
        let _ownership=(service,permit);
        let mut snapshot=snapshot;
        let error_tx=tx.clone();
        let result=(|| -> io::Result<()> {
            let mut encoder=zstd::stream::write::Encoder::new(Writer {tx,runtime},0)?;
            io::copy(&mut snapshot.file,&mut encoder)?;
            encoder.finish()?;
            Ok(())
        })();
        if result.is_err() { let _=error_tx.try_send(Err(io::Error::other("native snapshot stream failed"))); }
    });
    let mut response=Response::new(Body::new(Stream(rx)));
    response.headers_mut().insert("anki-original-size",size.to_string().parse().expect("integer header"));
    Ok(response)
}

struct Retained { response:Body,_incoming:Body,_permit:Option<OwnedSemaphorePermit>,_owner:Arc<Service> }
impl HttpBody for Retained {
    type Data=Bytes;
    type Error=axum::Error;
    fn poll_frame(mut self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<Option<std::result::Result<Frame<Bytes>,axum::Error>>> {
        Pin::new(&mut self.response).poll_frame(cx)
    }
}
pub(crate) fn retain(response:Response,incoming:Body,permit:Option<OwnedSemaphorePermit>,owner:Arc<Service>)->Response {
    let (mut parts,body)=response.into_parts();
    parts.headers.insert("connection","close".parse().expect("static header"));
    Response::from_parts(parts,Body::new(Retained {response:body,_incoming:incoming,_permit:permit,_owner:owner}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shutdown_during_incomplete_upload_releases_owned_stage_and_reservations() {
        struct Pending {chunk:Option<Bytes>,arrived:Option<tokio::sync::oneshot::Sender<()>>}
        impl HttpBody for Pending {
            type Data=Bytes;type Error=io::Error;
            fn poll_frame(mut self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<Option<io::Result<Frame<Bytes>>>> {
                if let Some(bytes)=self.chunk.take() {return Poll::Ready(Some(Ok(Frame::data(bytes))));}
                if let Some(tx)=self.arrived.take() {let _=tx.send(());}
                Poll::Pending
            }
        }
        let temp=tempfile::tempdir().unwrap();let mut root=crate::DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
        let token=std::fs::read_to_string(root.setup_file()).unwrap();
        let service=Service::open(root,crate::Config::new("https://example.test","127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let identity=service.claim("127.0.0.1".parse().unwrap(),"setup",token.trim().into(),"synthetic","synthetic password".into()).await.unwrap();
        let id=identity.id.clone();let account=temp.path().join("owner/state/accounts").join(&id);
        let before=std::fs::read(account.join("current-collection")).unwrap();
        let metadata=service.temp.reserve(4096).unwrap();let stage=service.metadata.files(move |r|r.stage_collection(&id)).await.unwrap();let staged_path=stage.path.clone();
        let compressed=zstd::stream::encode_all(vec![0;65536].as_slice(),0).unwrap();
        let (arrived,rx)=tokio::sync::oneshot::channel();let mut body=Body::new(Pending {chunk:Some(Bytes::copy_from_slice(&compressed[..compressed.len()/2])),arrived:Some(arrived)});
        let budget=service.temp.clone();let owned=service.clone();
        let work=tokio::spawn(async move {receive(owned,&mut body,move |reader,limit|super::super::full::receive(reader,limit,budget,stage,metadata)).await});
        rx.await.unwrap();service.native_ingress.close();
        assert!(work.await.unwrap().is_err());
        assert!(!staged_path.exists());assert_eq!(service.temp.used(),0);
        assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),before);
    }
}
