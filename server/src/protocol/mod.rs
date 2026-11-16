// SPDX-License-Identifier: MIT
pub mod framing;
pub mod login;
pub mod meta;
pub(crate) mod full;
pub(crate) mod normal;
pub(crate) mod media;
use crate::{Error, Result, Service, collection::schema};
use axum::{body::Body, extract::{ConnectInfo, Request}, http::{Method, StatusCode}, response::{IntoResponse,Response}};
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};

fn error(e:Error)->Response {
    let status=match e.code {
        "authentication_failed" | "stale_authority" => StatusCode::FORBIDDEN,
        "invalid_collection" | "invalid_request" | "validation_failed" => StatusCode::BAD_REQUEST,
        "unsupported_normal_case" => StatusCode::BAD_REQUEST,
        "invalid_session" | "missing_media" => StatusCode::CONFLICT,
        _ => e.status(),
    };
    let mut response=(status,e.code).into_response();
    if let Some(seconds)=e.retry_after { response.headers_mut().insert("retry-after",seconds.to_string().parse().expect("integer header")); }
    response
}
fn header(parts:&axum::http::request::Parts)->Result<login::Header> {
    let mut values=parts.headers.get_all("anki-sync").iter();
    let value=values.next().ok_or(Error::new("invalid_request"))?.as_bytes();
    if values.next().is_some() || value.iter().any(|b|!(0x20..=0x7e).contains(b)) { return Err(Error::new("invalid_request")); }
    let header:login::Header=framing::json(value)?;
    if header.v!=11 { return Err(Error::new("invalid_request")); }
    Ok(header)
}
const METHODS:&[&str]=&["hostKey","meta","upload","download","start","applyGraves","applyChanges","chunk","applyChunk","sanityCheck2","finish","abort"];
pub(crate) async fn handle(service:Arc<Service>,request:Request)->Response {
    let (parts,body)=request.into_parts();
    // Native names have no alternate route or SPA behavior.
    let media_route=parts.uri.path().starts_with("/msync/");
    let route=if media_route {parts.uri.path().strip_prefix("/msync/").filter(|s|media::METHODS.contains(s))}
        else {parts.uri.path().strip_prefix("/sync/").filter(|s|METHODS.contains(s))};
    let Some(method)=route else {
        return framing::retain(error(Error::new("route_not_found")),body,None,service);
    };
    let method=method.to_owned();
    if parts.method!=Method::POST {
        let mut response=(StatusCode::METHOD_NOT_ALLOWED,"method_not_allowed").into_response();
        response.headers_mut().insert("allow","POST".parse().expect("static header"));
        return framing::retain(response,body,None,service);
    }
    let permit=match service.native_ingress.enter() {
        Ok(p)=>p,Err(e)=>return framing::retain(error(e),body,None,service),
    };
    let worker=tokio::spawn(async move {
        let mut body=body;
        let response=dispatch(service.clone(),&parts,&method,&mut body).await.unwrap_or_else(error);
        framing::retain(response,body,Some(permit),service)
    });
    worker.await.unwrap_or_else(|_|error(Error::new("internal_error")))
}
#[derive(Deserialize)]
struct Empty {}
async fn dispatch(service:Arc<Service>,parts:&axum::http::request::Parts,method:&str,body:&mut Body)->Result<Response> {
    if parts.uri.query().is_some() || parts.headers.contains_key("content-encoding") { return Err(Error::new("invalid_request")); }
    let header=header(parts)?;
    let media_route=parts.uri.path().starts_with("/msync/");
    if !media_route && method=="hostKey" {
        let bytes=framing::receive(service.clone(),body,move |reader,limit| {
            let mut bytes=Vec::new(); framing::decode(reader,&mut bytes,limit)?; Ok(bytes)
        }).await?;
        let login:login::Login=framing::json(&bytes)?;
        let socket=parts.extensions.get::<ConnectInfo<SocketAddr>>().ok_or(Error::new("internal_error"))?.0;
        let (ip,_)=crate::httpapi::effective_peer(&parts.headers,socket,&service);
        let (_,key)=service.native_login(ip,&login.u,login.p).await?;
        return framing::response(&serde_json::to_vec(&serde_json::json!({"key":key})).map_err(|_|Error::new("internal_error"))?);
    }
    let authority=service.validate_hostkey(&header.k).await?;
    if media_route {return media::handle(service,authority,method,body).await;}
    if !matches!(method,"upload"|"download"|"meta") {
        return normal::handle(service,authority,header.s,method.to_owned(),body).await;
    }
    let capacity=service.collections.acquire()?;
    match method {
        "upload" => {
            // Streaming/storage path selection only follows native authority.
            let id=authority.account_id().to_owned();
            let metadata_reservation=service.temp.reserve(4096)?;
            let generation=service.metadata.files(move |root|root.stage_collection(&id)).await?;
            let budget=service.temp.clone();
            let upload=framing::receive(service.clone(),body,move |reader,limit| {
                let _capacity=capacity;
                full::receive(reader,limit,budget,generation,metadata_reservation)
            }).await?;
            full::publish(service,authority,upload).await?;
            framing::response(b"OK")
        },
        "meta" | "download" => {
            let bytes=framing::receive(service.clone(),body,move |reader,limit| {
                let mut bytes=Vec::new(); framing::decode(reader,&mut bytes,limit)?; Ok(bytes)
            }).await?;
            if method=="meta" {
                let request:meta::Request=framing::json(&bytes)?;
                if request.v!=11 { return Err(Error::new("invalid_request")); }
                let id=authority.account_id().to_owned();
                let authority_for_media=authority.clone();
                let result=service.collection_operation(authority,false,move |root| {
                    let _capacity=capacity;
                    meta::read(&schema::open(&root.collection_path(&id)?,false)?)
                }).await?;
                let mut result=result;
                let id=authority_for_media.account_id().to_owned();
                result.media_usn=service.publish(authority_for_media,move |c|crate::media::index::cursor(c,&id)).await?;
                framing::response(&serde_json::to_vec(&result).map_err(|_|Error::new("internal_error"))?)
            } else {
                let _:Empty=framing::json(&bytes)?;
                let snapshot=full::snapshot(service.clone(),authority,capacity).await?;
                framing::download(service,snapshot)
            }
        },
        _ => Err(Error::new("route_not_found")),
    }
}
