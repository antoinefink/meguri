//! Neutral input checks only. Docker ownership and independent acceptance are
//! proved by the orchestration owner, outside this client's namespace.
use crate::{Result, need, sha};
use serde::Deserialize;
use serde_json::Value;
use std::{fs::{self, File, OpenOptions}, io::Read, os::{fd::AsRawFd, unix::fs::{MetadataExt, OpenOptionsExt}}, path::Path};

pub fn json(path: impl AsRef<Path>) -> Result<Value> { Ok(serde_json::from_slice(&fs::read(path)?)?) }
pub fn keys(v: &Value, names: &[&str]) -> Result<()> {
    let object = v.as_object().ok_or("object-required")?;
    need(object.len() == names.len() && names.iter().all(|n| object.contains_key(*n)), "object-keys")
}
pub fn hash(v: &Value) -> Result<&str> {
    let s = v.as_str().ok_or("hash-type")?;
    need(s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "hash-shape")?;
    Ok(s)
}
pub fn artifact(v: &Value) -> Result<Vec<u8>> {
    let digest = hash(v)?;
    let data = fs::read(Path::new("/in/artifacts").join(digest))?;
    need(sha(&data) == digest, "artifact-hash")?;
    Ok(data)
}
fn artifact_json(v: &Value) -> Result<Value> { Ok(serde_json::from_slice(&artifact(v)?)?) }

pub fn preflight() -> Result<Value> {
    let peer = json("/in/preflight.json")?;
    keys(&peer, &["format", "kind", "server", "source_dirty", "profile", "features", "executable_artifact_sha256",
        "lock_artifact_sha256", "mg_build_receipt_sha256", "build_invocation", "execution_binding_sha256", "package"])?;
    need(peer["format"] == "meguri-sync-package-v1" && peer["kind"] == "final_package_preflight"
        && peer["source_dirty"] == false && peer["profile"] == "release" && peer["features"] == serde_json::json!([]), "before-only-preflight-required")?;
    let server = &peer["server"];
    need(server["kind"] == "meguri_sync" && server["executable_sha256"] == peer["executable_artifact_sha256"]
        && server["lock_sha256"] == peer["lock_artifact_sha256"], "server-identity")?;
    artifact(&peer["executable_artifact_sha256"])?;
    artifact(&peer["lock_artifact_sha256"])?;
    let build = artifact_json(&peer["mg_build_receipt_sha256"])?;
    keys(&peer["build_invocation"], &["job", "run_id", "arguments"])?;
    need(build["job"] == "sync-server-package" && build["status"] == "pass" && build["dry_run"] == false
        && build["args"] == serde_json::json!([]) && build["commit"]["dirty"] == false
        && build["commit"]["sha"] == server["source_commit"] && build["root"].as_str().is_some()
        && peer["build_invocation"]["job"] == build["job"] && peer["build_invocation"]["run_id"] == build["run_id"]
        && peer["build_invocation"]["arguments"] == build["args"], "package-build-receipt")?;
    let package = &peer["package"];
    keys(package, &["image_id", "inputs_sha256", "runtime_inventory_sha256", "image_inspection_sha256", "assets_sha256"])?;
    let image = artifact_json(&package["image_inspection_sha256"])?;
    let inputs = artifact_json(&package["inputs_sha256"])?;
    let inventory = artifact_json(&package["runtime_inventory_sha256"])?;
    let assets = artifact(&package["assets_sha256"])?;
    need(image.as_array().is_some_and(|a| a.len() == 1) && image[0]["Id"] == package["image_id"]
        && image[0]["Config"]["User"] == "10001:10001"
        && image[0]["Config"]["Entrypoint"] == serde_json::json!(["/usr/local/bin/meguri-sync-server"])
        && build["details"]["package"]["image_id"] == package["image_id"]
        && build["details"]["package"]["inputs_sha256"] == package["inputs_sha256"]
        && build["details"]["package"]["runtime_inventory_sha256"] == package["runtime_inventory_sha256"], "immutable-package")?;
    need(inputs["source_commit"] == server["source_commit"] && inputs["profile"] == "release" && inputs["features"] == serde_json::json!([])
        && inputs["source_archive_sha256"] == server["source_sha256"]
        && inventory["usr/local/bin/meguri-sync-server"]["sha256"] == server["executable_sha256"], "package-source-executable")?;
    artifact(&inputs["source_archive_sha256"])?;
    let locks: Vec<_> = inputs["inputs"].as_array().ok_or("package-inputs")?.iter()
        .filter(|i| i["path"] == "server/Cargo.lock").collect();
    need(locks.len() == 1 && locks[0]["sha256"] == server["lock_sha256"], "package-lock-identity")?;
    // assets.json is shipped content, not an unrelated well-formed manifest.
    let assets_entries: Vec<_> = inventory.as_object().ok_or("runtime-inventory")?.iter()
        .filter(|(p, _)| p.ends_with("/assets.json")).map(|(_, v)| v).collect();
    need(assets_entries.len() == 1 && assets_entries[0]["sha256"] == sha(&assets), "shipped-assets")?;
    let binding = artifact_json(&peer["execution_binding_sha256"])?;
    keys(&binding, &["format", "endpoint", "server", "run_id", "container_id", "labels", "observed_executable_sha256",
        "package", "server_process", "container_inspection_before_sha256"])?;
    need(binding["format"] == "meguri-sync-candidate-execution-v1" && binding["endpoint"] == crate::ENDPOINT
        && binding["server"] == *server && binding["package"] == *package
        && binding["run_id"] == std::env::var("MG_RUN_ID")?
        && binding["observed_executable_sha256"] == server["executable_sha256"]
        && binding["labels"]["meguri.mg.run-id"] == binding["run_id"], "execution-binding")?;
    hash(&binding["container_id"])?;
    keys(&binding["server_process"], &["pid", "executable_sha256"])?;
    need(binding["server_process"]["pid"].as_u64().is_some_and(|n| n > 0 && n <= 2_147_483_647)
        && binding["server_process"]["executable_sha256"] == server["executable_sha256"], "running-process")?;
    let inspection = artifact_json(&binding["container_inspection_before_sha256"])?;
    need(inspection.as_array().is_some_and(|a| a.len() == 1) && inspection[0]["Id"] == binding["container_id"]
        && inspection[0]["Image"] == package["image_id"] && inspection[0]["Config"]["User"] == "10001:10001"
        && inspection[0]["Config"]["Labels"]["meguri.mg.run-id"] == binding["run_id"]
        && inspection[0]["HostConfig"]["ReadonlyRootfs"] == true, "owned-container-inspection")?;
    Ok(peer)
}
pub fn invocation(peer: &Value, input: &Value, action: &[u8]) -> Result<Value> {
    let v = json("/in/invocation.json")?;
    keys(&v, &["format", "run_id", "root", "server_arguments", "data_volume", "owner_nonce",
        "case", "action", "actor", "client_build_sha256", "input_manifest_sha256", "preflight_peer_sha256"])?;
    need(v["format"] == "meguri-retained-invocation-v1" && v["run_id"] == std::env::var("MG_RUN_ID")?
        && v["run_id"].as_str().is_some_and(|s| !s.is_empty())
        && v["case"] == input["case"] && v["action"] == input["action"] && v["actor"] == "client_a"
        && v["client_build_sha256"] == sha(&fs::read("/in/client-build.json")?)
        && v["input_manifest_sha256"] == sha(&fs::read("/in/input-manifest.json")?)
        && v["preflight_peer_sha256"] == sha(&fs::read("/in/preflight.json")?)
        && input["action_sha256"] == sha(action), "retained-invocation")?;
    let build = artifact_json(&peer["mg_build_receipt_sha256"])?;
    need(v["root"].as_str().is_some_and(|s| s.starts_with('/')) && v["root"] == build["root"], "canonical-root")?;
    let args = v["server_arguments"].as_array().ok_or("server-arguments")?;
    need(args.len() == 6 && args.iter().all(Value::is_string) && args[0] == "serve" && args[1] == "/data/owner"
        && args[2].as_str().is_some_and(|s| s.starts_with("https://")) && args[3] == "0.0.0.0:8080"
        && args[4] == "--trusted-peer" && args[5].as_str().is_some_and(|s| s.parse::<std::net::IpAddr>().is_ok()), "serve-arguments")?;
    let nonce = v["owner_nonce"].as_str().ok_or("owner-nonce")?;
    need(nonce.len() == 32 && nonce.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && v["data_volume"].as_str().is_some_and(|s| !s.is_empty()), "owned-volume-nonce")?;
    let binding = artifact_json(&peer["execution_binding_sha256"])?;
    let inspection = artifact_json(&binding["container_inspection_before_sha256"])?;
    let c = &inspection[0];
    need(c["Config"]["Entrypoint"] == serde_json::json!(["/usr/local/bin/meguri-sync-server"])
        && c["Config"]["Cmd"] == v["server_arguments"] && c["Args"] == v["server_arguments"]
        && c["Path"] == "/usr/local/bin/meguri-sync-server"
        && c["Config"]["Labels"] == binding["labels"]
        && binding["labels"]["meguri.sync.owner"] == v["owner_nonce"]
        && c["Created"].as_str().is_some_and(|s| !s.is_empty()) && c["State"]["Running"] == true
        && c["State"]["StartedAt"].as_str().is_some_and(|s| !s.is_empty()), "owned-serve-process")?;
    let mounts: Vec<_> = c["Mounts"].as_array().ok_or("owned-mounts")?.iter()
        .filter(|m| m["Destination"] == "/data").collect();
    need(mounts.len() == 1 && mounts[0]["Type"] == "volume" && mounts[0]["Name"] == v["data_volume"]
        && mounts[0]["Destination"] == "/data" && mounts[0]["RW"] == true, "owned-data-mount")?;
    Ok(v)
}
pub fn client_build() -> Result<Value> {
    let build = json("/in/client-build.json")?;
    need(build["format"] == "meguri-retained-build-v1"
        && build["executable_sha256"] == sha(&fs::read(std::env::current_exe()?)?)
        && build["recipe_sha256"] == sha(crate::fixture::RECIPES)
        && build["selection_sha256"] == sha(crate::fixture::SELECTION), "client-build-identity")?;
    artifact(&build["source_sha256"])?;
    artifact(&build["core_lock_sha256"])?;
    artifact(&build["peer_lock_sha256"])?;
    artifact(&build["resolved_graph_sha256"])?;
    Ok(build)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials { pub username: String, pub password: String }
pub fn credentials() -> Result<Credentials> {
    let path = Path::new("/secret/credentials.json");
    let before = fs::symlink_metadata(path)?;
    need(before.is_file() && before.mode() & 0o7777 == 0o600 && before.nlink() == 1, "credential-file")?;
    let mut file: File = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)?;
    let actual = file.metadata()?;
    // SAFETY: geteuid has no preconditions; fstatvfs writes one initialized output.
    let uid = unsafe { libc::geteuid() };
    need(before.dev() == actual.dev() && before.ino() == actual.ino() && actual.uid() == uid
        && actual.is_file() && actual.mode() & 0o7777 == 0o600 && actual.nlink() == 1, "credential-owner")?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let rc = unsafe { libc::fstatvfs(file.as_raw_fd(), stat.as_mut_ptr()) };
    need(rc == 0, "credential-mount")?;
    let stat = unsafe { stat.assume_init() };
    need(stat.f_flag & libc::ST_RDONLY != 0, "credential-readonly-mount")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let credentials: Credentials = serde_json::from_slice(&bytes)?;
    need(!credentials.username.trim().is_empty() && !credentials.password.is_empty(), "credential-shape")?;
    Ok(credentials)
}
