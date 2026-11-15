// SPDX-License-Identifier: MIT
mod support;
use meguri_sync_server::protocol::framing;
use serde::Deserialize;
use serde_json::json;
use support::*;

#[derive(Debug,Deserialize,PartialEq)]
struct Exact {id:i64,usn:i32}
#[test]
fn strict_integer_tokens_duplicates_depth_and_surrogates() {
    let value:Exact=framing::json(br#"{"id":9223372036854775807,"usn":2147483647}"#).unwrap();
    assert_eq!(value,Exact {id:i64::MAX,usn:i32::MAX});
    for bytes in [br#"{"id":9007199254740993.0,"usn":0}"#.as_slice(),br#"{"id":1e0,"usn":0}"#,br#"{"id":"1","usn":0}"#,br#"{"id":1,"id":2,"usn":0}"#,br#"{"id":1,"usn":2147483648}"#,br#"{"id":1,"usn":0,"unknown":"\ud800"}"#] {
        assert!(framing::json::<Exact>(bytes).is_err());
    }
    let too_deep=format!("{}0{}","[".repeat(128),"]".repeat(128));
    assert!(framing::json::<serde_json::Value>(too_deep.as_bytes()).is_err());
}
#[test]
fn raw_zstd_concatenation_truncation_trailing_data_and_decoded_limit() {
    let first=zstd::stream::encode_all(b"abc".as_slice(),0).unwrap();
    let second=zstd::stream::encode_all(b"def".as_slice(),0).unwrap();
    let mut joined=first.clone(); joined.extend_from_slice(&second);
    let mut out=Vec::new();
    framing::decode(joined.as_slice(),&mut out,6).unwrap();
    assert_eq!(out,b"abcdef");
    let mut out=Vec::new();
    assert_eq!(framing::decode(joined.as_slice(),&mut out,5).unwrap_err().code,"request_too_large");
    assert!(out.len()<=5);
    for broken in [&first[..first.len()-1],b"not zstd".as_slice()] {
        assert!(framing::decode(broken,&mut Vec::new(),100).is_err());
    }
    let mut trailing=first; trailing.push(0);
    assert!(framing::decode(trailing.as_slice(),&mut Vec::new(),100).is_err());
    // A protocol-skippable frame followed by an ordinary frame is valid.
    let mut skip=vec![0x50,0x2a,0x4d,0x18,1,0,0,0,0xff];skip.extend(second);
    let mut out=Vec::new();framing::decode(skip.as_slice(),&mut out,3).unwrap();assert_eq!(out,b"def");
}
#[tokio::test]
async fn native_header_and_cookie_boundaries_remain_distinct() {
    let h=Harness::new().await;h.setup().await;
    let cookie=h.login("root.admin",PASSWORD).await;
    let bytes=zstd::stream::encode_all(br#"{"v":11,"cv":"synthetic"}"#.as_slice(),0).unwrap();
    for header in [None,Some(r#"{"v":11.0,"k":"","c":"x","s":"x"}"#),Some(r#"{"v":11,"v":11,"k":"","c":"x","s":"x"}"#)] {
        let mut request=h.client.post(format!("{}/sync/meta",h.base)).header("cookie",&cookie).body(bytes.clone());
        if let Some(header)=header {request=request.header("anki-sync",header);}
        let response=request.send().await.unwrap();assert_eq!(response.status(),400);
        assert!(response.headers().get("anki-original-size").is_none());
    }
    let response=h.client.post(format!("{}/sync/meta",h.base))
        .header("cookie",&cookie).header("anki-sync",json!({"v":11,"k":secret(&cookie),"c":"x","s":"x"}).to_string()).body(bytes).send().await.unwrap();
    assert_eq!(response.status(),403);assert_eq!(response.text().await.unwrap(),"authentication_failed");
    let response=h.get("/sync/download",Some(&cookie)).await;
    assert_eq!(response.status(),405);assert_eq!(response.headers()["allow"],"POST");
    let host=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap().1;
    error(h.post("/api/admin/invitations",json!({}),Some(&format!("__Host-meguri-session={host}"))).await,401,"authentication_failed").await;
}
