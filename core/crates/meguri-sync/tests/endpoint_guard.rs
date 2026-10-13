//! The sync endpoint guard (R1876): every sync endpoint a test, harness or
//! scenario uses must be loopback, a private address, a Docker compose name or
//! a name under the reserved `.test` TLD, and an AnkiWeb host is refused by
//! name. The guard decides from the text alone, so a refused endpoint never
//! reaches DNS or a socket. meguri-testkit re-exports this guard; these tests
//! are the rule's only test vectors.
//!
//! No AnkiWeb host is written out here (the gate's static scan, U19): the
//! cases build it from the guard's own constant.

use meguri_sync::endpoint_guard::{ANKIWEB_HOST, Allowed, Reason, check_endpoint};

fn ankiweb(template: &str) -> String {
    template.replace("{ankiweb}", ANKIWEB_HOST)
}

#[track_caller]
fn refused_as(endpoint: &str, expected: Reason) {
    match check_endpoint(endpoint) {
        Ok(kind) => panic!("{endpoint:?} was allowed as {kind:?}"),
        Err(refused) => {
            assert_eq!(refused.reason, expected, "{endpoint:?}: {refused}");
            assert_eq!(refused.endpoint, endpoint);
        }
    }
}

#[track_caller]
fn allowed_as(endpoint: &str, expected: Allowed) {
    match check_endpoint(endpoint) {
        Ok(kind) => assert_eq!(kind, expected, "{endpoint:?}"),
        Err(refused) => panic!("{endpoint:?} was refused: {refused}"),
    }
}

#[test]
fn ankiweb_hosts_are_refused_by_name() {
    for template in [
        "https://{ankiweb}/",
        "https://sync.{ankiweb}/",
        "https://sync22.{ankiweb}/sync/meta",
        "http://sync3.{ankiweb}:80/msync/",
        "sync.{ankiweb}",
        "sync.{ankiweb}:443",
        "HTTPS://Sync.{ankiweb}/",
        "https://sync.{ankiweb}./",
        "https://sync.{ankiweb}:notaport/",
        "http://127.0.0.1@sync.{ankiweb}/",
        "http://user:pw@{ankiweb}:443/",
    ] {
        let endpoint = ankiweb(template);
        refused_as(&endpoint, Reason::AnkiWeb);
    }
    // Case is folded before the comparison.
    refused_as(
        &format!("https://SYNC.{}/", ANKIWEB_HOST.to_ascii_uppercase()),
        Reason::AnkiWeb,
    );
}

#[test]
fn a_lookalike_of_the_ankiweb_name_is_not_ankiweb_but_still_refused() {
    // Not AnkiWeb by name, and not local either.
    refused_as(&ankiweb("https://not{ankiweb}/"), Reason::NotLocal);
    refused_as(&ankiweb("https://{ankiweb}.example/"), Reason::NotLocal);
}

#[test]
fn compose_service_names_and_loopback_are_allowed() {
    allowed_as("syncserver:8080", Allowed::ComposeName);
    allowed_as("http://syncserver:8080/", Allowed::ComposeName);
    allowed_as("http://syncserver:8080", Allowed::ComposeName);
    allowed_as("https://toxiproxy:8081/sync/", Allowed::ComposeName);
    allowed_as("http://mitmdump:8080/", Allowed::ComposeName);
    // Containers started with `compose run` have generated single-label names.
    allowed_as(
        "http://meguri-u13-1234567-oracle-run-5f2a9c:8080/",
        Allowed::ComposeName,
    );
    allowed_as("http://sync_server/", Allowed::ComposeName);
    allowed_as("127.0.0.1", Allowed::Loopback);
    allowed_as("http://127.0.0.1:27701/", Allowed::Loopback);
    allowed_as("http://127.8.9.10/", Allowed::Loopback);
    allowed_as("http://localhost:8080/", Allowed::Loopback);
    allowed_as("http://LocalHost/", Allowed::Loopback);
    allowed_as("http://[::1]:8080/", Allowed::Loopback);
    allowed_as("[::1]", Allowed::Loopback);
}

#[test]
fn private_ranges_are_allowed_and_their_neighbours_are_not() {
    for endpoint in [
        "http://10.0.0.1/",
        "http://10.255.255.255/",
        "http://172.16.0.1:8080/",
        "http://172.31.255.255/",
        "http://192.168.0.1/",
        "http://192.168.1.66:27701/",
        "http://[fd00::1]/",
        "http://[fc00::5]:8080/",
    ] {
        allowed_as(endpoint, Allowed::PrivateRange);
    }
    for endpoint in [
        "http://172.15.255.255/",
        "http://172.32.0.1/",
        "http://192.169.0.1/",
        "http://11.0.0.1/",
        "http://8.8.8.8/",
        "http://100.64.0.1/",
        "http://169.254.169.254/",
        "http://0.0.0.0/",
        "http://255.255.255.255/",
        "http://[2001:db8::1]/",
        "http://[fe80::1]/",
        "http://[::]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[::ffff:10.0.0.1]/",
    ] {
        refused_as(endpoint, Reason::NotLocal);
    }
}

#[test]
fn names_under_the_reserved_test_tld_are_allowed() {
    for endpoint in [
        // The stand-in AnkiWeb suffix and its numbered sync hosts (U105).
        "https://sync20.meguri-sync.test/",
        "https://meguri-sync.test/sync/meta",
        "http://stub.test:8443/sync/",
        "stub.test",
        "stub.test:443",
        "https://a.b-c.d_e.test/",
        "HTTPS://Sync20.Stub.TEST/",
    ] {
        allowed_as(endpoint, Allowed::ReservedTestName);
    }
    for endpoint in [
        // A trailing dot, an empty label or another TLD is not a test name.
        "http://stub.test./",
        "http://.test/",
        "http://a..test/",
        "http://stub.testing/",
        "http://stub.test.example.com/",
        "http://test.example/",
    ] {
        refused_as(endpoint, Reason::NotLocal);
    }
    // The rest of the rule still applies to test names.
    refused_as("http://user@stub.test/", Reason::Malformed);
    refused_as("http://stub.test:0/", Reason::Malformed);
    refused_as("ftp://stub.test/", Reason::Malformed);
}

#[test]
fn public_and_dotted_names_are_refused() {
    for endpoint in [
        "https://example.com/",
        "http://syncserver.example.com:8080/",
        "http://localhost.example.com/",
        "http://syncserver.local/",
        "http://sub.localhost/",
        // A host that is all digits or 0x-prefixed is an IPv4 number to a
        // WHATWG URL parser (2130706433 is 127.0.0.1, 134744072 is 8.8.8.8),
        // so it is never read as a name.
        "http://2130706433/",
        "http://134744072/",
        "http://0x7f000001/",
        "http://9lives/",
        // Shorthand, octal and trailing-dot IPv4 forms parse differently
        // between URL parsers; only the strict dotted quad is read.
        "http://127.1/",
        "http://0177.0.0.1/",
        "http://010.0.0.1/",
        "http://127.0.0.1./",
        "http://localhost./",
    ] {
        refused_as(endpoint, Reason::NotLocal);
    }
}

#[test]
fn ambiguous_or_malformed_endpoints_are_refused() {
    for endpoint in [
        "",
        "http://",
        "http:///sync/",
        "http://:8080/",
        "ftp://127.0.0.1/",
        "file:///tmp/x",
        "ws://127.0.0.1/",
        " http://127.0.0.1/",
        "http://127.0.0.1:99999/",
        "http://127.0.0.1:/",
        "http://127.0.0.1:80a/",
        "http://user@127.0.0.1/",
        "http://user:pw@syncserver:8080/",
        "http://syncserver\\@example.com/",
        "http://sync\tserver/",
        "http://sync%73erver/",
        "http://ｓyncserver/",
        "http://[::1/",
        "http://[::1]x/",
        "http://[::1%25lo0]/",
        "http://[127.0.0.1]/",
    ] {
        refused_as(endpoint, Reason::Malformed);
    }
}

#[test]
fn the_refusal_names_the_endpoint_and_the_rule() {
    let refused = check_endpoint(&ankiweb("https://sync.{ankiweb}/")).unwrap_err();
    let text = refused.to_string();
    assert!(text.contains(&ankiweb("sync.{ankiweb}")), "{text}");
    assert!(text.contains("R1876"), "{text}");
    let refused = check_endpoint("https://example.com/").unwrap_err();
    assert!(refused.to_string().contains("example.com"), "{refused}");
}
