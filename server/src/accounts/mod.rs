// SPDX-License-Identifier: MIT
mod administration;
mod barrier;
mod credentials;
mod deletion;
mod reset;
mod backups;
pub use backups::Backup;
pub(crate) use backups::RestoreCaller;
mod provision;
pub(crate) mod oauth;
use crate::{
    Config, DataRoot, Error, Result, crypto,
    limits::{Attempts, CollectionCapacity, HashPool, TempBudget},
    storage::Metadata,
};
pub use barrier::{PublicationBarrier, RoundRetirer};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{net::IpAddr, sync::Arc};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub username: String,
    pub role: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct AccountSummary {
    #[serde(flatten)]
    pub identity: Identity,
    pub enabled: bool,
    pub provisioning_state: String,
    pub storage: Option<AccountStorage>,
}
#[derive(Clone, Debug, Serialize)]
pub struct AccountStorage {
    pub collection_bytes: u64,
    pub media_bytes: u64,
    pub total_bytes: u64,
}
#[derive(Clone)]
pub(crate) struct Account {
    pub identity: Identity,
    pub enabled: bool,
    pub state: String,
    pub phc: String,
    pub revision: i64,
    pub generation: i64,
}
impl Account {
    fn usable(&self) -> bool {
        self.enabled && self.state == "ready"
    }
}
#[derive(Clone)]
pub struct BrowserAuthority {
    pub(crate) account: Account,
    pub(crate) hash: [u8; 32],
}
impl BrowserAuthority {
    pub fn identity(&self) -> &Identity {
        &self.account.identity
    }
}
/// Constructed only by native password issuance or hostkey validation. Browser
/// cookies never construct native authority; the protocol owns header decoding.
#[derive(Clone, Debug)]
pub struct NativeAuthority {
    pub(crate) id: String,
    pub(crate) generation: i64,
    pub(crate) data_epoch: i64,
}
impl NativeAuthority {
    pub fn account_id(&self) -> &str {
        &self.id
    }
    pub fn credential_generation(&self) -> i64 {
        self.generation
    }
}

pub struct Service {
    pub config: Config,
    pub temp: TempBudget,
    pub collections: CollectionCapacity,
    pub(crate) browser_ingress: Arc<crate::httpapi::ingress::Ingress>,
    pub(crate) native_ingress: Arc<crate::protocol::framing::Ingress>,
    pub(crate) normal_sessions: Arc<crate::collection::session::Sessions>,
    pub(crate) mcp_key: [u8; 32],
    pub(crate) mcp_search_cache: Arc<crate::collection::mcp::SearchCache>,
    pub(crate) mcp_exports: Arc<crate::mcp_service::export::Exports>,
    pub(crate) mcp_uploads: Arc<crate::mcp_service::attachments::Uploads>,
    pub(crate) metadata: Metadata,
    pub(crate) hashes: HashPool,
    pub(crate) attempts: Attempts,
    pub(crate) barriers: barrier::Barriers,
    #[cfg(feature = "test-hooks")]
    pub hooks: Arc<crate::test_hooks::Hooks>,
}
impl Drop for Service {
    fn drop(&mut self) { self.normal_sessions.close(); }
}
impl Service {
    /// Ownership remains held by metadata and every outstanding blocking job.
    /// Startup recovery never allocates identities or retries expired authority.
    pub async fn open(root: DataRoot, config: Config) -> Result<Arc<Self>> {
        let root = Arc::new(root);
        let metadata = Metadata::new(root);
        metadata.files(|r| r.check_initialized()).await?;
        let service = Arc::new(Self {
            temp: TempBudget::new(config.temp_bytes),
            config,
            collections: CollectionCapacity::default(),
            browser_ingress: Arc::new(crate::httpapi::ingress::Ingress::default()),
            native_ingress: Arc::new(crate::protocol::framing::Ingress::default()),
            normal_sessions: crate::collection::session::Sessions::new(),
            mcp_key: crypto::bearer(&crypto::random_token()?)?,
            mcp_search_cache: Arc::new(crate::collection::mcp::SearchCache::default()),
            mcp_exports: Arc::new(crate::mcp_service::export::Exports::default()),
            mcp_uploads: Arc::new(crate::mcp_service::attachments::Uploads::default()),
            metadata,
            hashes: HashPool::default(),
            attempts: Attempts::default(),
            barriers: barrier::Barriers::default(),
            #[cfg(feature = "test-hooks")]
            hooks: Arc::new(crate::test_hooks::Hooks::default()),
        });
        service.recover_backup_restores().await?;
        service.recover_resets().await?;
        if service.admission_open().await? {
            service.resume_startup().await?;
        }
        // Provisioning must obtain its staging budget before any schema write.
        // The exclusive root owner still prevents serving until both complete;
        // upgrade_media validates semantics before and after migration.
        service.metadata.files(|r| {
            r.upgrade_media()?;
            r.upgrade_deletions()?;
            r.upgrade_oauth()?;
            r.upgrade_resets()?;
            r.upgrade_backups()?;
            Ok(())
        }).await?;
        service.recover_deletions().await?;
        if service.admission_open().await? {
            service.metadata.files(crate::media::recovery::startup).await?;
        }
        Ok(service)
    }
    pub async fn status(&self) -> Result<bool> {
        self.metadata
            .run(|c| {
                admission(c)?;
                Ok(c.query_row(
                    "SELECT phase<>'closed' FROM setup WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await
    }
    pub async fn admission_open(&self) -> Result<bool> {
        self.metadata
            .run(|c| {
                Ok(c.query_row(
                    "SELECT admission_open FROM service_state WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await
    }
    /// Restore must close admission durably before installing restored data.
    /// Deliberately no ordinary remote/recovery method opens admission.
    pub async fn close_admission(&self) -> Result<()> {
        self.metadata
            .run(|c| {
                c.execute(
                    "UPDATE service_state SET admission_open=0 WHERE singleton=1",
                    [],
                )?;
                Ok(())
            })
            .await
    }
    pub(crate) async fn pause(&self, name: &'static str) {
        #[cfg(feature = "test-hooks")]
        self.hooks.hit(name).await;
        #[cfg(not(feature = "test-hooks"))]
        let _ = name;
    }
    pub async fn me(&self, authority: BrowserAuthority) -> Result<Identity> {
        self.metadata
            .run(move |c| {
                admission(c)?;
                Ok(check_browser(c, &authority, "authentication_failed")?.identity)
            })
            .await
    }
    pub(crate) fn name_attempt(&self, ip: IpAddr, name: &str) -> Result<String> {
        // Invalid names still consume a bounded digest bucket; never retain the
        // supplied name in a limiter key. Login maps validation to generic auth.
        let canonical = name
            .trim_matches(|c: char| c.is_ascii_whitespace())
            .to_ascii_lowercase();
        self.attempts.username(ip, &canonical)?;
        crypto::username(&canonical)
    }
}
pub(crate) fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
pub(crate) fn admission(c: &Connection) -> Result<()> {
    if !c.query_row(
        "SELECT admission_open FROM service_state WHERE singleton=1",
        [],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(Error::new("admission_closed"));
    }
    Ok(())
}
pub(crate) fn read_account(c: &Connection, id: &str) -> Result<Option<Account>> {
    c.query_row("SELECT id,username,role,enabled,provisioning_state,password_phc,password_revision,generation FROM accounts WHERE id=?1",[id], |r| {
        Ok(Account { identity:Identity{id:r.get(0)?,username:r.get(1)?,role:r.get(2)?},enabled:r.get(3)?,state:r.get(4)?,phc:r.get(5)?,revision:r.get(6)?,generation:r.get(7)? })
    }).optional().map_err(Into::into)
}
pub(crate) fn check_browser(
    c: &Connection,
    a: &BrowserAuthority,
    failure: &'static str,
) -> Result<Account> {
    Ok(check_browser_expiry(c, a, failure, now())?.0)
}
pub(crate) fn check_browser_expiry(
    c: &Connection,
    a: &BrowserAuthority,
    failure: &'static str,
    checked_at: i64,
) -> Result<(Account, i64)> {
    let current = read_account(c, &a.account.identity.id)?
        .filter(|r| r.usable() && r.generation == a.account.generation)
        .ok_or(Error::new(failure))?;
    let stored:Option<(Vec<u8>,i64)>=c.query_row("SELECT hash,expires_at FROM credentials WHERE hash=?1 AND account_id=?2 AND kind='session' AND revoked=0 AND generation=?3 AND expires_at>?4",
        params![a.hash.as_slice(),current.identity.id,current.generation,checked_at],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let (hash, expires_at) = stored.ok_or(Error::new(failure))?;
    if !crypto::same_digest(&hash, &a.hash) {
        return Err(Error::new(failure));
    }
    Ok((current, expires_at))
}
pub(crate) fn check_admin(
    c: &Connection,
    a: &BrowserAuthority,
    failure: &'static str,
) -> Result<Account> {
    let current = check_browser(c, a, failure)?;
    if current.identity.role != "admin" {
        return Err(Error::new("permission_denied"));
    }
    Ok(current)
}
pub(crate) fn revoke(c: &Connection, id: &str) -> Result<()> {
    c.execute("UPDATE credentials SET revoked=1 WHERE account_id=?1", [id])?;
    c.execute("UPDATE capabilities SET state='revoked' WHERE state IN ('available','provisioning') AND (issuer=?1 OR target=?1)",[id])?;
    oauth::revoke_account(c, id)?;
    Ok(())
}
pub(crate) fn replace_password(c: &Connection, id: &str, phc: &str) -> Result<()> {
    c.execute("UPDATE accounts SET password_phc=?2,password_revision=password_revision+1,generation=generation+1 WHERE id=?1",params![id,phc])?;
    revoke(c, id)
}
pub(crate) fn phase(name: &str) -> Result<()> {
    #[cfg(feature = "test-hooks")]
    crate::test_hooks::process_phase(name)?;
    #[cfg(not(feature = "test-hooks"))]
    let _ = name;
    Ok(())
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    #[tokio::test]
    async fn every_metadata_connection_verifies_durability_and_capacity_before_spawn() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        root.init().unwrap();
        let service = Service::open(
            root,
            Config::new("https://example.test", "127.0.0.1:0".parse().unwrap()).unwrap(),
        )
        .await
        .unwrap();
        service
            .metadata
            .run(|c| {
                assert_eq!(
                    c.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))?,
                    2
                );
                assert_eq!(
                    c.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))?,
                    1
                );
                assert_eq!(
                    c.pragma_query_value(None, "busy_timeout", |r| r.get::<_, i64>(0))?,
                    5000
                );
                assert_eq!(
                    c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))?,
                    "wal"
                );
                Ok(())
            })
            .await
            .unwrap();
        let (arrived, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let mut jobs = Vec::new();
        for _ in 0..8 {
            let s = service.clone();
            let arrived = arrived.clone();
            let release = release.clone();
            jobs.push(tokio::spawn(async move {
                s.metadata
                    .run(move |_| {
                        arrived.send(()).unwrap();
                        let (m, c) = &*release;
                        let mut go = m.lock().unwrap();
                        while !*go {
                            go = c.wait(go).unwrap();
                        }
                        Ok(())
                    })
                    .await
            }));
        }
        for _ in 0..8 {
            rx.recv().await.unwrap();
        }
        assert_eq!(
            service.metadata.run(|_| Ok(())).await.unwrap_err().code,
            "storage_unavailable"
        );
        *release.0.lock().unwrap() = true;
        release.1.notify_all();
        for job in jobs {
            job.await.unwrap().unwrap();
        }
    }
    #[tokio::test]
    async fn invalid_capability_rejected_even_with_hash_admission_exhausted() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        root.init().unwrap();
        let token = std::fs::read_to_string(root.setup_file())
            .unwrap()
            .trim()
            .to_owned();
        let service = Service::open(
            root,
            Config::new("https://example.test", "127.0.0.1:0".parse().unwrap()).unwrap(),
        )
        .await
        .unwrap();
        // The same pool's own concurrency test checks the running/queued jobs;
        // here take admission directly inside the defining module's test scope.
        let held = service.hashes.test_hold_admission();
        let invalid = crypto::random_token().unwrap();
        let ip = "127.0.0.1".parse().unwrap();
        assert_eq!(
            service
                .claim(ip, "setup", invalid, "admin", "valid long password".into())
                .await
                .unwrap_err()
                .code,
            "capability_invalid"
        );
        assert_eq!(
            service
                .claim(ip, "setup", token, "admin", "valid long password".into())
                .await
                .unwrap_err()
                .code,
            "rate_limited"
        );
        drop(held);
    }
    #[tokio::test]
    async fn login_hash_admission_is_uniform_and_dummy_proof_cannot_issue() {
        let temp = tempfile::tempdir().unwrap();
        let mut root = DataRoot::acquire(&temp.path().join("owner"), true).unwrap();
        root.init().unwrap();
        let token = std::fs::read_to_string(root.setup_file())
            .unwrap()
            .trim()
            .to_owned();
        let service = Service::open(
            root,
            Config::new("https://example.test", "127.0.0.1:0".parse().unwrap()).unwrap(),
        )
        .await
        .unwrap();
        let ip = "127.0.0.1".parse().unwrap();
        let account = service
            .claim(ip, "setup", token, "admin", "valid long password".into())
            .await
            .unwrap();
        for enabled in [true, false] {
            let id = account.id.clone();
            service
                .metadata
                .run(move |c| {
                    c.execute(
                        "UPDATE accounts SET enabled=?1 WHERE id=?2",
                        params![enabled, id],
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
            let admission = service.hashes.test_hold_admission();
            // Finite pool fixture: no sockets or concurrent request generator.
            for name in [" ADMIN ", "unknown"] {
                for native in [false, true] {
                    let result = if native {
                        service
                            .native_login(ip, name, "valid long password".into())
                            .await
                    } else {
                        service
                            .browser_login(ip, name, "valid long password".into())
                            .await
                    };
                    let error = result.unwrap_err();
                    assert_eq!(error.code, "rate_limited");
                    assert_eq!(error.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
                    assert!(error.retry_after.is_some());
                }
            }
            drop(admission);
        }
        // This public synthetic password matches the dummy PHC. It still must
        // not create a session or hostkey for either unknown or disabled names.
        for name in ["admin", "unknown"] {
            for native in [false, true] {
                let result = if native {
                    service
                        .native_login(ip, name, "compatibility-password-2026".into())
                        .await
                } else {
                    service
                        .browser_login(ip, name, "compatibility-password-2026".into())
                        .await
                };
                assert_eq!(result.unwrap_err().code, "authentication_failed");
            }
        }
        assert_eq!(
            service
                .metadata
                .run(
                    |c| Ok(c.query_row("SELECT count(*) FROM credentials", [], |r| r
                        .get::<_, i64>(0))?)
                )
                .await
                .unwrap(),
            0
        );
    }
}
