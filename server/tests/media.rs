// SPDX-License-Identifier: MIT
mod support;

use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use support::*;

async fn native(h: &Harness, family: &str, method: &str, key: &str, bytes: &[u8]) -> reqwest::Response {
    h.client
        .post(format!("{}/{family}/{method}", h.base))
        .header("anki-sync", json!({"v":11,"k":key,"c":"synthetic-media","s":""}).to_string())
        .body(zstd::stream::encode_all(bytes, 0).unwrap())
        .send().await.unwrap()
}

async fn decoded(response: reqwest::Response) -> Vec<u8> {
    assert_eq!(response.status(), 200);
    assert!(response.headers().get("content-encoding").is_none());
    let length: usize = response.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
    let bytes = response.bytes().await.unwrap();
    let result = zstd::stream::decode_all(bytes.as_ref()).unwrap();
    assert_eq!(result.len(), length);
    result
}

async fn login(h: &Harness) -> String {
    let request = serde_json::to_vec(&json!({"u":"root.admin","p":PASSWORD})).unwrap();
    let bytes = decoded(native(h, "sync", "hostKey", "", &request).await).await;
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let key = value["key"].as_str().unwrap();
    assert!(!key.is_empty());
    key.into()
}

async fn media(h: &Harness, key: &str, method: &str, bytes: &[u8]) -> Value {
    let bytes = decoded(native(h, "msync", method, key, bytes).await).await;
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(envelope["err"], "");
    envelope.as_object().unwrap().get("data").unwrap().clone()
}

async fn media_json(h: &Harness, key: &str, method: &str, value: Value) -> Value {
    media(h, key, method, &serde_json::to_vec(&value).unwrap()).await
}

async fn begin(h: &Harness, key: &str, usn: i32) {
    let result = media_json(h, key, "begin", json!({"v":"synthetic-media"})).await;
    assert_eq!(result["usn"], usn);
    assert_eq!(result["sk"], "");
}

async fn changes(h: &Harness, key: &str, cursor: i32) -> Value {
    media_json(h, key, "mediaChanges", json!({"lastUsn":cursor})).await
}

// Tiny, test-only classic ZIP writer: stored members, CRC-32, central directory
// and EOCD. All names/lengths are fixture-owned; this is not an upload parser.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn get16(bytes: &[u8], offset: usize) -> usize {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize
}
fn get32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut directory = Vec::new();
    for (name, content) in entries {
        let offset: u32 = bytes.len().try_into().unwrap();
        let length: u32 = content.len().try_into().unwrap();
        let name_length: u16 = name.len().try_into().unwrap();
        let crc = crc32(content);
        let mut local = [0; 30];
        put32(&mut local, 0, 0x0403_4b50);
        put16(&mut local, 4, 20); // version needed
        put16(&mut local, 12, 33); // DOS date: 1980-01-01
        put32(&mut local, 14, crc);
        put32(&mut local, 18, length);
        put32(&mut local, 22, length);
        put16(&mut local, 26, name_length);
        bytes.extend_from_slice(&local);
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(content);

        let mut central = [0; 46];
        put32(&mut central, 0, 0x0201_4b50);
        put16(&mut central, 4, 20); // version made by
        put16(&mut central, 6, 20); // version needed
        put16(&mut central, 14, 33);
        put32(&mut central, 16, crc);
        put32(&mut central, 20, length);
        put32(&mut central, 24, length);
        put16(&mut central, 28, name_length);
        put32(&mut central, 42, offset);
        directory.extend_from_slice(&central);
        directory.extend_from_slice(name.as_bytes());
    }
    let mut end = [0; 22];
    put32(&mut end, 0, 0x0605_4b50);
    put16(&mut end, 8, entries.len().try_into().unwrap());
    put16(&mut end, 10, entries.len().try_into().unwrap());
    put32(&mut end, 12, directory.len().try_into().unwrap());
    put32(&mut end, 16, bytes.len().try_into().unwrap());
    bytes.extend(directory);
    bytes.extend_from_slice(&end);
    bytes
}

fn addition(name: &str, content: &[u8]) -> Vec<u8> {
    let meta = serde_json::to_vec(&json!([[name,"0"]])).unwrap();
    stored_zip(&[("0", content), ("_meta", &meta)])
}
fn deletion(name: &str) -> Vec<u8> {
    let meta = serde_json::to_vec(&json!([[name,null]])).unwrap();
    stored_zip(&[("_meta", &meta)])
}

// Inspect the specified stored download format, using central sizes so either
// local-header lengths or standard data descriptors work. No member is extracted.
fn stored_members(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let end = (0..=bytes.len().checked_sub(22).unwrap()).rev().find(|&at| {
        get32(bytes, at) == 0x0605_4b50 && at + 22 + get16(bytes, at + 20) == bytes.len()
    }).unwrap();
    assert_eq!(get16(bytes, end + 4), 0);
    assert_eq!(get16(bytes, end + 6), 0);
    let count = get16(bytes, end + 10);
    assert_eq!(get16(bytes, end + 8), count);
    let mut at = get32(bytes, end + 16) as usize;
    let directory_end = at + get32(bytes, end + 12) as usize;
    assert_eq!(directory_end, end);
    let mut members = BTreeMap::new();
    for _ in 0..count {
        assert_eq!(get32(bytes, at), 0x0201_4b50);
        assert_eq!(get16(bytes, at + 8) & 1, 0, "unencrypted download");
        assert_eq!(get16(bytes, at + 10), 0, "stored download member");
        let length = get32(bytes, at + 24) as usize;
        assert_eq!(get32(bytes, at + 20) as usize, length);
        let name_length = get16(bytes, at + 28);
        let name = std::str::from_utf8(&bytes[at + 46..at + 46 + name_length]).unwrap();
        let local = get32(bytes, at + 42) as usize;
        assert_eq!(get32(bytes, local), 0x0403_4b50);
        let local_name_length = get16(bytes, local + 26);
        assert_eq!(&bytes[local + 30..local + 30 + local_name_length], name.as_bytes());
        let start = local + 30 + local_name_length + get16(bytes, local + 28);
        let content = bytes[start..start + length].to_vec();
        assert_eq!(crc32(&content), get32(bytes, at + 16));
        assert!(members.insert(name.to_owned(), content).is_none(), "duplicate ZIP member");
        at += 46 + name_length + get16(bytes, at + 30) + get16(bytes, at + 32);
    }
    assert_eq!(at, directory_end);
    members
}

async fn download(h: &Harness, key: &str, names: &[&str]) -> BTreeMap<String, Vec<u8>> {
    let request = serde_json::to_vec(&json!({"files":names})).unwrap();
    let bytes = decoded(native(h, "msync", "downloadFiles", key, &request).await).await;
    let mut members = stored_members(&bytes);
    let meta: BTreeMap<String, String> = serde_json::from_slice(&members.remove("_meta").unwrap()).unwrap();
    assert_eq!(meta.len(), names.len());
    assert_eq!(members.len(), names.len());
    let mut result = BTreeMap::new();
    for (index, name) in names.iter().enumerate() {
        let member = index.to_string();
        assert_eq!(meta.get(&member).map(String::as_str), Some(*name));
        let content = members.remove(&member).unwrap();
        assert!(!content.is_empty());
        assert!(result.insert((*name).to_owned(), content).is_none());
    }
    result
}

#[tokio::test]
async fn authenticated_media_upload_storage_tracks_live_blobs_per_account() {
    let h = Harness::new().await;
    let identity = h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "media.member").await;
    let member_key = h.service.native_login(IP, "media.member", PASSWORD.into()).await.unwrap().1;
    let key = login(&h).await;
    let member_cookie = h.login("media.member", PASSWORD).await;
    async fn assert_sizes(h: &Harness, admin: &str, member_cookie: &str, id: &str, member_id: &str, bytes: u64) {
        let response = h.get("/api/admin/accounts", Some(admin)).await;
        assert_eq!(response.status(), 200);
        let list = response.json::<Value>().await.unwrap();
        let accounts = list["data"]["accounts"].as_array().unwrap();
        for (id, expected) in [(id, bytes), (member_id, 5)] {
            let storage = &accounts.iter().find(|a| a["id"] == id).unwrap()["storage"];
            let cookie = if id == member_id { member_cookie } else { admin };
            let own = h.get("/api/me/storage", Some(cookie)).await;
            assert_eq!(own.status(), 200);
            let own = own.json::<Value>().await.unwrap();
            assert_eq!(&own["data"], storage);
            assert_eq!(own["data"].as_object().unwrap().len(), 3);
            assert_eq!(storage["media_bytes"], expected);
            assert_eq!(storage["total_bytes"].as_u64().unwrap(), storage["collection_bytes"].as_u64().unwrap() + expected);
        }
    }
    // Same live name in another account must never contribute to this account.
    assert_eq!(media(&h, &member_key, "uploadChanges", &addition("one.bin", b"other")).await, json!([1,1]));
    let id = identity["id"].as_str().unwrap();
    let member_id = member["id"].as_str().unwrap();
    assert_sizes(&h, &admin, &member_cookie, id, member_id, 0).await;
    assert_eq!(media(&h, &key, "uploadChanges", &addition("one.bin", &[0xff])).await, json!([1,1]));
    assert_sizes(&h, &admin, &member_cookie, id, member_id, 1).await;
    assert_eq!(media(&h, &key, "uploadChanges", &addition("one.bin", b"replace")).await, json!([1,2]));
    assert_sizes(&h, &admin, &member_cookie, id, member_id, 7).await;
    assert_eq!(media(&h, &key, "uploadChanges", &deletion("one.bin")).await, json!([1,3]));
    assert_sizes(&h, &admin, &member_cookie, id, member_id, 0).await;
    let _ = h.stop().await;
}

#[tokio::test]
async fn media_changes_download_delete_and_noop_round_trip() {
    let h = Harness::new().await;
    h.setup().await;
    let key = login(&h).await;
    let name = "round-trip.bin";
    let original = b"original synthetic media\0\xff\x80\r\n";
    let upload = addition(name, original);
    let live = json!([[name,1,Sha1::digest(original).iter().map(|byte| format!("{byte:02x}")).collect::<String>()]]);
    assert_eq!(media(&h, &key, "uploadChanges", &upload).await, json!([1,1]));
    begin(&h, &key, 1).await;
    assert_eq!(changes(&h, &key, 0).await, live);
    assert_eq!(changes(&h, &key, 1).await, json!([]));
    assert_eq!(download(&h, &key, &[name]).await[name], original);
    assert_eq!(media(&h, &key, "uploadChanges", &upload).await, json!([1,1]));
    assert_eq!(media(&h, &key, "uploadChanges", &deletion("absent.bin")).await, json!([1,1]));
    assert_eq!(changes(&h, &key, 0).await, live);
    assert_eq!(media_json(&h, &key, "mediaSanity", json!({"local":1})).await, "OK");
    assert_eq!(media_json(&h, &key, "mediaSanity", json!({"local":0})).await, "mediaSanity");

    let delete = deletion(name);
    assert_eq!(media(&h, &key, "uploadChanges", &delete).await, json!([1,2]));
    assert_eq!(media(&h, &key, "uploadChanges", &delete).await, json!([1,2]));
    begin(&h, &key, 2).await;
    for cursor in [0, 1] {
        assert_eq!(changes(&h, &key, cursor).await, json!([[name,2,""]]));
    }
    assert_eq!(changes(&h, &key, 2).await, json!([]));
    assert_eq!(media_json(&h, &key, "mediaSanity", json!({"local":0})).await, "OK");
    let request = serde_json::to_vec(&json!({"files":[name]})).unwrap();
    assert_eq!(native(&h, "msync", "downloadFiles", &key, &request).await.status(), 409);
    assert!(download(&h, &key, &[]).await.is_empty());
    let bytes = decoded(native(&h, "sync", "meta", &key, br#"{"v":11,"cv":"synthetic-media"}"#).await).await;
    let meta: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(meta["media_usn"], 2);
    assert_eq!(meta["usn"], 0);
    let _ = h.stop().await;
}

#[tokio::test]
async fn acknowledged_media_bytes_cursor_and_tombstone_survive_service_restart() {
    let h = Harness::new().await;
    h.setup().await;
    let key = login(&h).await;
    let original = b"retained across restart\0\xfe\xff\x80";
    let meta = serde_json::to_vec(&json!([["retained.bin","0"],["removed.bin","1"]])).unwrap();
    let upload = stored_zip(&[("0", original), ("1", b"discarded synthetic bytes"), ("_meta", &meta)]);
    assert_eq!(media(&h, &key, "uploadChanges", &upload).await, json!([2,2]));
    assert_eq!(media(&h, &key, "uploadChanges", &deletion("removed.bin")).await, json!([1,3]));
    // Restart the real service over its original root. No state reconstruction
    // from the sender or inspection of a proposed on-disk media layout.
    let temp = h.stop().await;
    let h = Harness::reopen(temp).await;
    begin(&h, &key, 3).await;
    assert_eq!(changes(&h, &key, 0).await, json!([
        ["retained.bin",1,Sha1::digest(original).iter().map(|byte| format!("{byte:02x}")).collect::<String>()],
        ["removed.bin",3,""]
    ]));
    assert_eq!(changes(&h, &key, 3).await, json!([]));
    assert_eq!(download(&h, &key, &["retained.bin"]).await["retained.bin"], original);
    assert_eq!(media_json(&h, &key, "mediaSanity", json!({"local":1})).await, "OK");
    assert_eq!(media(&h, &key, "uploadChanges", &addition("retained.bin", original)).await, json!([1,3]));
    assert_eq!(media(&h, &key, "uploadChanges", &deletion("removed.bin")).await, json!([1,3]));
    let request = br#"{"files":["retained.bin","removed.bin"]}"#;
    assert_eq!(native(&h, "msync", "downloadFiles", &key, request).await.status(), 409);
    let _ = h.stop().await;
}

#[tokio::test]
async fn invalid_media_upload_preserves_committed_state_across_restart() {
    let h = Harness::new().await;
    h.setup().await;
    let key = login(&h).await;
    let original = b"committed before invalid batch\0\xff";
    assert_eq!(media(&h, &key, "uploadChanges", &addition("kept.bin", original)).await, json!([1,1]));
    let before = changes(&h, &key, 0).await;
    // The first change is valid and would replace committed bytes. A missing
    // member for the second addition must reject the entire batch.
    let meta = serde_json::to_vec(&json!([["kept.bin","0"],["missing.bin","1"]])).unwrap();
    let invalid = stored_zip(&[("0", b"must never replace committed bytes"), ("_meta", &meta)]);
    assert_eq!(native(&h, "msync", "uploadChanges", &key, &invalid).await.status(), 400);
    begin(&h, &key, 1).await;
    assert_eq!(changes(&h, &key, 0).await, before);
    assert_eq!(download(&h, &key, &["kept.bin"]).await["kept.bin"], original);
    assert_eq!(media_json(&h, &key, "mediaSanity", json!({"local":1})).await, "OK");
    let temp = h.stop().await;
    let h = Harness::reopen(temp).await;
    begin(&h, &key, 1).await;
    assert_eq!(changes(&h, &key, 0).await, before);
    assert_eq!(changes(&h, &key, 1).await, json!([]));
    assert_eq!(download(&h, &key, &["kept.bin"]).await["kept.bin"], original);
    let request = br#"{"files":["missing.bin"]}"#;
    assert_eq!(native(&h, "msync", "downloadFiles", &key, request).await.status(), 409);
    let _ = h.stop().await;
}

// Additional independently authored proofs. The four seed proofs above retain
// their original assertions and identities.
fn batch(items:&[(String,Option<Vec<u8>>)],method:zip::CompressionMethod)->Vec<u8> {
    use std::io::Write;
    let mut archive=zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options=zip::write::SimpleFileOptions::default().compression_method(method);
    let mut meta=Vec::new();
    for (position,(name,content)) in items.iter().enumerate() {
        let key=content.as_ref().map(|_|position.to_string());
        meta.push((name,key.clone()));
        if let Some(content)=content {
            archive.start_file(key.unwrap(),options).unwrap();
            archive.write_all(content).unwrap();
        }
    }
    archive.start_file("_meta",options).unwrap();
    archive.write_all(&serde_json::to_vec(&meta).unwrap()).unwrap();
    archive.finish().unwrap().into_inner()
}
fn digest(bytes:&[u8])->String {Sha1::digest(bytes).iter().map(|b|format!("{b:02x}")).collect()}

#[tokio::test]
async fn media_1005_files_paginate_and_26_names_split_into_legal_25_and_1() {
    let h=Harness::new().await;h.setup().await;let key=login(&h).await;
    let files=(0..1005).map(|n|(format!("file-{n:04}.bin"),Some(format!("payload {n}").into_bytes()))).collect::<Vec<_>>();
    let mut usn=0;
    for items in files.chunks(25) {
        usn+=items.len();
        assert_eq!(media(&h,&key,"uploadChanges",&batch(items,zip::CompressionMethod::Stored)).await,json!([items.len(),usn]));
    }
    let first=changes(&h,&key,0).await;let second=changes(&h,&key,1000).await;
    assert_eq!(first.as_array().unwrap().len(),1000);assert_eq!(second.as_array().unwrap().len(),5);
    for (position,row) in first.as_array().unwrap().iter().chain(second.as_array().unwrap()).enumerate() {
        assert_eq!(*row,json!([files[position].0,position+1,digest(files[position].1.as_ref().unwrap())]));
    }
    assert_eq!(changes(&h,&key,1005).await,json!([]));assert_eq!(changes(&h,&key,i32::MAX).await,json!([]));
    assert_eq!(native(&h,"msync","mediaChanges",&key,br#"{"lastUsn":-1}"#).await.status(),400);
    let names=files[..26].iter().map(|v|v.0.as_str()).collect::<Vec<_>>();
    let request=serde_json::to_vec(&json!({"files":names})).unwrap();
    assert_eq!(native(&h,"msync","downloadFiles",&key,&request).await.status(),400);
    for part in [&names[..25],&names[25..]] {
        let result=download(&h,&key,part).await;
        for name in part {let index=files.iter().position(|v|v.0==*name).unwrap();assert_eq!(result[*name],*files[index].1.as_ref().unwrap());}
    }
    let _=h.stop().await;
}

#[tokio::test]
async fn mixed_repeated_names_case_variants_and_deflate_preserve_sequential_noops() {
    let h=Harness::new().await;h.setup().await;let key=login(&h).await;
    let items=vec![
        ("Same.bin".into(),Some(b"first".to_vec())),
        ("Same.bin".into(),Some(b"first".to_vec())),
        ("Same.bin".into(),None),
        ("absent.bin".into(),None),
        ("Same.bin".into(),Some(b"last".to_vec())),
        ("same.bin".into(),Some(b"lower case".to_vec())),
    ];
    assert_eq!(media(&h,&key,"uploadChanges",&batch(&items,zip::CompressionMethod::Deflated)).await,json!([6,4]));
    assert_eq!(changes(&h,&key,0).await,json!([["Same.bin",3,digest(b"last")],["same.bin",4,digest(b"lower case")]]));
    let replay=vec![("Same.bin".into(),Some(b"last".to_vec())),("absent.bin".into(),None),("same.bin".into(),Some(b"lower case".to_vec()))];
    assert_eq!(media(&h,&key,"uploadChanges",&batch(&replay,zip::CompressionMethod::Stored)).await,json!([3,4]));
    let received=download(&h,&key,&["Same.bin","same.bin"]).await;
    assert_eq!(received["Same.bin"],b"last");assert_eq!(received["same.bin"],b"lower case");
    let _=h.stop().await;
}

#[tokio::test]
async fn download_fill_crossing_member_and_missing_beyond_prefix_are_atomic() {
    let h=Harness::new().await;h.setup().await;let key=login(&h).await;
    let items=(0..5).map(|n|(format!("one-mib-{n}"),Some(vec![n as u8;1024*1024]))).collect::<Vec<_>>();
    assert_eq!(media(&h,&key,"uploadChanges",&batch(&items,zip::CompressionMethod::Deflated)).await,json!([5,5]));
    let names=items.iter().map(|v|v.0.as_str()).collect::<Vec<_>>();
    let request=serde_json::to_vec(&json!({"files":names})).unwrap();
    let mut members=stored_members(&decoded(native(&h,"msync","downloadFiles",&key,&request).await).await);
    let meta:Value=serde_json::from_slice(&members.remove("_meta").unwrap()).unwrap();
    assert_eq!(meta,json!({"0":names[0],"1":names[1],"2":names[2]}));assert_eq!(members.len(),3);
    for n in 0..3 {assert_eq!(members[&n.to_string()],*items[n].1.as_ref().unwrap());}
    let mut missing=names.clone();missing.push("missing-after-fill");
    let request=serde_json::to_vec(&json!({"files":missing})).unwrap();
    assert_eq!(native(&h,"msync","downloadFiles",&key,&request).await.status(),409);
    let _=h.stop().await;
}

#[tokio::test]
async fn invalid_archive_matrix_rejects_whole_batch_and_retains_prior_bytes() {
    let h=Harness::new().await;h.setup().await;let key=login(&h).await;
    assert_eq!(media(&h,&key,"uploadChanges",&addition("kept",b"old")).await,json!([1,1]));
    let before=changes(&h,&key,0).await;
    let meta=br#"[["kept","0"]]"#;
    let mut cases=Vec::new();
    cases.push(stored_zip(&[("0",b"new"),("0",b"duplicate"),("_meta",meta)]));
    cases.push(stored_zip(&[("0",b"new"),("_meta",meta),("_meta",meta)]));
    cases.push(stored_zip(&[("0",b"new"),("1",b"unreferenced"),("_meta",meta)]));
    cases.push(stored_zip(&[("x",b"new"),("_meta",meta)]));
    cases.push(stored_zip(&[("0",b"new"),("_meta",br#"[["kept","0"],["other","0"]]"#)]));
    cases.push(stored_zip(&[("0",b"new"),("_meta",br#"[["kept","1"]]"#)]));
    cases.push(addition("zero",b""));
    cases.push(batch(&(0..26).map(|n|(format!("n{n}"),None)).collect::<Vec<_>>(),zip::CompressionMethod::Stored));
    cases.push(stored_zip(&[("_meta",&vec![b' ';50*1024+1])]));
    for name in ["",".","..","a/b","a\\b","a\0b","e\u{301}"] {cases.push(addition(name,b"new"));}
    cases.push(addition(&"x".repeat(256),b"new"));
    let mut bad=addition("kept",b"new");let data=30+get16(&bad,26)+get16(&bad,28);bad[data]^=1;cases.push(bad);
    let mut truncated=addition("kept",b"new");truncated.truncate(truncated.len()-9);cases.push(truncated);
    // Malicious central modes/method/CRC, while local bytes remain ordinary.
    for kind in [0,1,2,3] {
        let mut bad=addition("kept",b"new");let end=bad.len()-22;let central=get32(&bad,end+16) as usize;
        match kind {
            0=>{put16(&mut bad,central+4,0x0314);put32(&mut bad,central+38,(0o120777u32)<<16);},
            1=>put16(&mut bad,central+8,1),
            2=>put16(&mut bad,central+10,99),
            _=>put32(&mut bad,central+16,1),
        }
        cases.push(bad);
    }
    // Deflate declares and expands past the legal per-entry size. Synthetic
    // fixture lives only in this process, never in the source tree.
    cases.push(batch(&[("oversized".into(),Some(vec![0;104_857_601]))],zip::CompressionMethod::Deflated));
    for archive in cases {
        let r=native(&h,"msync","uploadChanges",&key,&archive).await;
        assert!(matches!(r.status().as_u16(),400|413));
        assert_eq!(changes(&h,&key,0).await,before);assert_eq!(download(&h,&key,&["kept"]).await["kept"],b"old");
        assert_eq!(h.service.temp.used(),0);
    }
    let _=h.stop().await;
}

#[tokio::test]
async fn legal_100_mib_entry_is_file_backed_durable_and_replay_is_noop() {
    let h=Harness::new().await;h.setup().await;let key=login(&h).await;
    let bytes=vec![0x5a;104_857_600];
    let archive=batch(&[("large".into(),Some(bytes.clone()))],zip::CompressionMethod::Deflated);
    assert_eq!(media(&h,&key,"uploadChanges",&archive).await,json!([1,1]));
    assert_eq!(h.service.temp.used(),0);
    let temp=h.stop().await;let h=Harness::reopen(temp).await;
    assert_eq!(download(&h,&key,&["large"]).await["large"],bytes);
    assert_eq!(media(&h,&key,"uploadChanges",&archive).await,json!([1,1]));
    assert_eq!(h.service.temp.used(),0);let _=h.stop().await;
}

#[tokio::test]
async fn corrupt_blob_refuses_download_and_identical_upload_without_hiding_metadata() {
    let h=Harness::new().await;let identity=h.setup().await;let key=login(&h).await;
    assert_eq!(media(&h,&key,"uploadChanges",&addition("kept",b"old")).await,json!([1,1]));
    let before=changes(&h,&key,0).await;
    let c=rusqlite::Connection::open(h.database()).unwrap();
    let blob:String=c.query_row("SELECT blob FROM media_names WHERE account_id=?1 AND name='kept'",[identity["id"].as_str().unwrap()],|r|r.get(0)).unwrap();drop(c);
    std::fs::write(h.path().join("state/accounts").join(identity["id"].as_str().unwrap()).join("media").join(blob),b"BAD").unwrap();
    assert_eq!(native(&h,"msync","downloadFiles",&key,br#"{"files":["kept"]}"#).await.status(),500);
    assert_eq!(native(&h,"msync","uploadChanges",&key,&addition("kept",b"old")).await.status(),500);
    assert_eq!(changes(&h,&key,0).await,before);begin(&h,&key,1).await;
    let _=h.stop().await;
}

#[tokio::test]
async fn exact_utf8_name_boundary_and_exhausted_usn_preserve_noop_and_atomicity() {
    let h=Harness::new().await;let identity=h.setup().await;let key=login(&h).await;
    let name=format!("{}a","é".repeat(127));assert_eq!(name.len(),255);
    assert_eq!(media(&h,&key,"uploadChanges",&addition(&name,b"kept")).await,json!([1,1]));
    let temp=h.stop().await;
    let root=meguri_sync_server::DataRoot::acquire(&temp.path().join("owner"),false).unwrap();
    let c=rusqlite::Connection::open(root.database()).unwrap();
    c.execute("UPDATE accounts SET media_usn=2147483647 WHERE id=?1",[identity["id"].as_str().unwrap()]).unwrap();drop(c);drop(root);
    let h=Harness::reopen(temp).await;
    assert_eq!(media(&h,&key,"uploadChanges",&addition(&name,b"kept")).await,json!([1,i32::MAX]));
    let input=vec![(name.clone(),None),("new".into(),Some(b"cannot fit".to_vec()))];
    let response=native(&h,"msync","uploadChanges",&key,&batch(&input,zip::CompressionMethod::Stored)).await;
    assert_eq!(response.status(),500);assert_eq!(response.text().await.unwrap(),"media_usn_exhausted");
    begin(&h,&key,i32::MAX).await;assert_eq!(download(&h,&key,&[&name]).await[&name],b"kept");
    assert_eq!(changes(&h,&key,0).await,json!([[name,1,digest(b"kept")]]));let _=h.stop().await;
}

// Convert only ZIPs produced by the fixture writer above. Preserve its vetted
// compressed payloads and finalized central records; emit the legal streaming
// form with bit 3, zero local CRC/sizes, and signed classic data descriptors.
// This is a small fixture writer transformation, never an upload parser.
fn descriptor_form(source: &[u8]) -> Vec<u8> {
    let end = source.len() - 22;
    assert_eq!(get32(source, end), 0x0605_4b50);
    let mut at = get32(source, end + 16) as usize;
    let mut bytes = Vec::new();
    let mut directory = Vec::new();
    for _ in 0..get16(source, end + 10) {
        assert_eq!(get32(source, at), 0x0201_4b50);
        let record_len = 46 + get16(source, at + 28) + get16(source, at + 30) + get16(source, at + 32);
        let mut central = source[at..at + record_len].to_vec();
        let offset = get32(&central, 42) as usize;
        assert_eq!(get32(source, offset), 0x0403_4b50);
        assert_eq!(get16(source, offset + 6) & 8, 0);
        let header_len = 30 + get16(source, offset + 26) + get16(source, offset + 28);
        let mut local = source[offset..offset + header_len].to_vec();
        let compressed = get32(&central, 20);
        let expanded = get32(&central, 24);
        let crc = get32(&central, 16);
        assert_eq!(get32(&local, 14), crc);
        assert_eq!(get32(&local, 18), compressed);
        assert_eq!(get32(&local, 22), expanded);
        put16(&mut local, 6, (get16(source, offset + 6) | 8) as u16);
        for field in [14, 18, 22] { put32(&mut local, field, 0); }
        let flags = (get16(&central, 8) | 8) as u16;
        put16(&mut central, 8, flags);
        put32(&mut central, 42, bytes.len().try_into().unwrap());
        bytes.extend_from_slice(&local);
        let payload = offset + header_len;
        bytes.extend_from_slice(&source[payload..payload + compressed as usize]);
        for value in [0x0807_4b50, crc, compressed, expanded] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        directory.extend_from_slice(&central);
        at += record_len;
    }
    assert_eq!(at, end);
    let mut footer = source[end..].to_vec();
    put32(&mut footer, 12, directory.len().try_into().unwrap());
    put32(&mut footer, 16, bytes.len().try_into().unwrap());
    bytes.extend_from_slice(&directory);
    bytes.extend_from_slice(&footer);
    bytes
}

async fn descriptor_upload_roundtrip(method: zip::CompressionMethod) {
    use std::io::Read;
    let original = b"descriptor payload\0\xff\x80\r\n";
    let items = vec![("descriptor.bin".into(), Some(original.to_vec()))];
    let source = batch(&items, method);
    let archive = descriptor_form(&source);
    // Check fixture validity independently with the vetted central reader,
    // including decoding/CRC, before sending it to our actual HTTP service.
    let mut checked = zip::ZipArchive::new(std::io::Cursor::new(&archive)).unwrap();
    assert_eq!(checked.len(), 2);
    for (member, expected) in [
        ("0", original.to_vec()),
        ("_meta", serde_json::to_vec(&json!([["descriptor.bin", "0"]])).unwrap()),
    ] {
        let mut entry = checked.by_name(member).unwrap();
        assert_eq!(entry.compression(), method);
        let mut actual = Vec::new();
        entry.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, expected);
    }
    let h = Harness::new().await;
    h.setup().await;
    let key = login(&h).await;
    assert_eq!(media(&h, &key, "uploadChanges", &archive).await, json!([1,1]));
    let listing = json!([["descriptor.bin", 1, digest(original)]]);
    begin(&h, &key, 1).await;
    assert_eq!(changes(&h, &key, 0).await, listing);
    assert_eq!(download(&h, &key, &["descriptor.bin"]).await["descriptor.bin"], original);
    assert_eq!(h.service.temp.used(), 0);
    let temp = h.stop().await;
    let h = Harness::reopen(temp).await;
    begin(&h, &key, 1).await;
    assert_eq!(changes(&h, &key, 0).await, listing);
    assert_eq!(changes(&h, &key, 1).await, json!([]));
    assert_eq!(download(&h, &key, &["descriptor.bin"]).await["descriptor.bin"], original);
    assert_eq!(media(&h, &key, "uploadChanges", &archive).await, json!([1,1]));
    assert_eq!(h.service.temp.used(), 0);
    let _ = h.stop().await;
}

// Acceptance requirements: expected to expose the sequential preflight gap.
// Actual red evidence belongs to the orchestrator's first MG run.
#[tokio::test]
async fn stored_data_descriptor_upload_preserves_exact_bytes_and_listing_after_restart() {
    descriptor_upload_roundtrip(zip::CompressionMethod::Stored).await;
}

#[tokio::test]
async fn deflated_data_descriptor_upload_preserves_exact_bytes_and_listing_after_restart() {
    descriptor_upload_roundtrip(zip::CompressionMethod::Deflated).await;
}

#[tokio::test]
async fn malformed_small_central_directory_and_zip64_metadata_reject_without_committed_effects() {
    let h = Harness::new().await;
    let identity = h.setup().await;
    let key = login(&h).await;
    assert_eq!(media(&h, &key, "uploadChanges", &addition("kept", b"original")).await, json!([1,1]));
    let before = changes(&h, &key, 0).await;
    let directory = h.path().join("state/accounts").join(identity["id"].as_str().unwrap()).join("media");
    let files = store_bytes(&directory);
    let source = addition("kept", b"must not replace original");
    let end = source.len() - 22;
    let central = get32(&source, end + 16) as usize;
    let size = get32(&source, end + 12);
    let mut cases = Vec::new();
    // Counts remain tiny; these are malformed own-server requests, not an
    // extreme-allocation probe or a standalone third-party crash reproducer.
    for count in [1, 3] {
        let mut bad = source.clone();
        put16(&mut bad, end + 8, count);
        put16(&mut bad, end + 10, count);
        cases.push((format!("declared count {count}"), bad));
    }
    let mut bad = source.clone();
    put16(&mut bad, end + 8, 1);
    cases.push(("inconsistent disk count".into(), bad));
    for declared in [size - 1, size + 1] {
        let mut bad = source.clone();
        put32(&mut bad, end + 12, declared);
        cases.push((format!("declared directory size {declared}"), bad));
    }
    // ZIP64 size sentinels require an extra field. Test missing and truncated
    // size metadata using otherwise tiny local payloads and directory records.
    let mut missing = source.clone();
    put16(&mut missing, central + 6, 45);
    put32(&mut missing, central + 20, u32::MAX);
    put32(&mut missing, central + 24, u32::MAX);
    cases.push(("missing ZIP64 size extra".into(), missing.clone()));
    let extra_at = central + 46 + get16(&missing, central + 28);
    assert_eq!(get16(&missing, central + 30), 0);
    let mut extra = vec![1, 0, 16, 0]; // ZIP64 extra declares two u64 sizes
    extra.extend_from_slice(&24u64.to_le_bytes()); // only one is present
    missing.splice(extra_at..extra_at, extra.iter().copied());
    put16(&mut missing, central + 30, extra.len().try_into().unwrap());
    put32(&mut missing, end + extra.len() + 12, size + extra.len() as u32);
    cases.push(("truncated ZIP64 size extra".into(), missing));
    for (label, archive) in cases {
        assert!(archive.len() < 1024);
        let response = native(&h, "msync", "uploadChanges", &key, &archive).await;
        assert_eq!(response.status(), 400, "{label}");
        assert_eq!(h.service.temp.used(), 0, "{label}");
        assert_eq!(store_bytes(&directory), files, "{label}");
        begin(&h, &key, 1).await;
        assert_eq!(changes(&h, &key, 0).await, before, "{label}");
        assert_eq!(download(&h, &key, &["kept"]).await["kept"], b"original", "{label}");
        assert_eq!(h.service.temp.used(), 0, "{label}");
    }
    let temp = h.stop().await;
    let h = Harness::reopen(temp).await;
    begin(&h, &key, 1).await;
    assert_eq!(changes(&h, &key, 0).await, before);
    assert_eq!(download(&h, &key, &["kept"]).await["kept"], b"original");
    let _ = h.stop().await;
}

#[tokio::test]
async fn configured_expanded_aggregate_boundary_rejects_whole_batch_and_cleans_temporary_files() {
    const LIMIT: usize = 8192;
    let h = Harness::new().await;
    let identity = h.setup().await;
    let key = login(&h).await;
    let mut config = meguri_sync_server::Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap();
    config.native_decompressed_bytes = std::num::NonZeroU64::new(LIMIT as u64).unwrap();
    let temp = h.stop().await;
    let h = Harness::reopen_with_config(temp, config.clone()).await;
    let meta_len = serde_json::to_vec(&json!([["kept", "0"], ["other", "1"]])).unwrap().len();
    let second = LIMIT - LIMIT / 2 - meta_len;
    let items = vec![
        ("kept".into(), Some(vec![0x61; LIMIT / 2])),
        ("other".into(), Some(vec![0x62; second])),
    ];
    let archive = batch(&items, zip::CompressionMethod::Deflated);
    assert!(archive.len() < LIMIT); // ZIP framing is below the separate decode limit
    // Exactly LIMIT expanded bytes, including _meta, are legal.
    assert_eq!(media(&h, &key, "uploadChanges", &archive).await, json!([2,2]));
    let before = changes(&h, &key, 0).await;
    let directory = h.path().join("state/accounts").join(identity["id"].as_str().unwrap()).join("media");
    let files = store_bytes(&directory);
    // First exceed the aggregate by one including metadata, then by file data
    // alone. Each individual file remains below the configured limit.
    for length in [second + 1, LIMIT / 2 + 1] {
        let rejected = vec![
            ("kept".into(), Some(vec![0x78; LIMIT / 2])),
            ("other".into(), Some(vec![0x79; length])),
        ];
        let archive = batch(&rejected, zip::CompressionMethod::Deflated);
        assert!(archive.len() < LIMIT);
        assert_eq!(native(&h, "msync", "uploadChanges", &key, &archive).await.status(), 413);
        assert_eq!(h.service.temp.used(), 0);
        assert_eq!(store_bytes(&directory), files);
        begin(&h, &key, 2).await;
        assert_eq!(changes(&h, &key, 0).await, before);
        let received = download(&h, &key, &["kept", "other"]).await;
        for (name, content) in &items { assert_eq!(&received[name], content.as_ref().unwrap()); }
        assert_eq!(h.service.temp.used(), 0);
    }
    let temp = h.stop().await;
    let h = Harness::reopen_with_config(temp, config).await;
    begin(&h, &key, 2).await;
    assert_eq!(changes(&h, &key, 0).await, before);
    let received = download(&h, &key, &["kept", "other"]).await;
    for (name, content) in &items { assert_eq!(&received[name], content.as_ref().unwrap()); }
    assert_eq!(h.service.temp.used(), 0);
    let _ = h.stop().await;
}
