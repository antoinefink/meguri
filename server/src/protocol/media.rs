// SPDX-License-Identifier: MIT
use crate::{Error, Result, Service, NativeAuthority, media::{self,archive,index}};
use super::framing;
use axum::{body::Body,response::Response};
use serde::{Deserialize,Serialize};
use std::sync::Arc;
#[derive(Deserialize)] struct Begin { v:String }
#[derive(Deserialize)] struct Changes { #[serde(rename="lastUsn")] last:i32 }
#[derive(Deserialize)] struct Sanity { local:u32 }
#[derive(Deserialize)] struct Files { files:Vec<String> }
fn response(value:&impl Serialize)->Result<Response> {
    framing::response(&serde_json::to_vec(&serde_json::json!({"data":value,"err":""})).map_err(|_|Error::new("internal_error"))?)
}
pub(crate) const METHODS:&[&str]=&["begin","mediaChanges","uploadChanges","downloadFiles","mediaSanity"];
pub(crate) async fn handle(service:Arc<Service>,authority:NativeAuthority,method:&str,body:&mut Body)->Result<Response> {
    if method=="uploadChanges" {
        let root=service.metadata.owner(); let id=authority.account_id().to_owned(); let budget=service.temp.clone();
        let changes=framing::receive(service.clone(),body,move |reader,limit|archive::receive(reader,limit,&root,&id,budget)).await?;
        return response(&media::upload(service,authority,changes).await?);
    }
    // These small request schemas have their own bound. Upload ZIP bytes never
    // enter a JSON Vec or the browser's 8 MiB body path.
    let bytes=framing::receive(service.clone(),body,move |reader,limit| {
        let mut bytes=Vec::new(); framing::decode(reader,&mut bytes,limit.min(64*1024))?; Ok(bytes)
    }).await?;
    let id=authority.account_id().to_owned();
    match method {
        "begin"=>{
            let request:Begin=framing::json(&bytes)?; let _=request.v;
            let usn=service.publish(authority,move |c|index::cursor(c,&id)).await?;
            response(&serde_json::json!({"usn":usn,"sk":""}))
        },
        "mediaChanges"=>{
            let request:Changes=framing::json(&bytes)?;
            let changes=service.publish(authority,move |c|index::changes(c,&id,request.last)).await?;
            response(&changes)
        },
        "mediaSanity"=>{
            let request:Sanity=framing::json(&bytes)?;
            let count=service.publish(authority,move |c|index::count(c,&id)).await?;
            response(&if count==request.local {"OK"} else {"mediaSanity"})
        },
        "downloadFiles"=>{
            let request:Files=framing::json(&bytes)?;
            if request.files.len()>25 {return Err(Error::new("invalid_request"));}
            for n in &request.files {media::name(n)?;}
            let account=id.clone();
            let requested=service.publish(authority,move |c| {
                let mut result=Vec::with_capacity(request.files.len());
                for n in request.files {
                    let content=index::state(c,&id,&n)?.and_then(|s|s.content).ok_or(Error::new("missing_media"))?;
                    result.push((n,content));
                }
                Ok(result)
            }).await?;
            service.pause("media_download_pinned").await;
            let root=service.metadata.owner(); let budget=service.temp.clone();
            let output=tokio::task::spawn_blocking(move ||archive::download(&root,&account,budget,requested)).await.map_err(|_|Error::new("storage_unavailable"))??;
            #[cfg(feature="test-hooks")]
            service.pause("media_download_opened").await;
            framing::file_download(service,output.file.try_clone()?,output)
        },
        _=>Err(Error::new("route_not_found")),
    }
}
