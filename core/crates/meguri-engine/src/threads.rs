//! The thread registry (`engine/jobs` J6, KTD9): every thread the core creates
//! goes through it, so each is named, runs at its QoS class from its first
//! instruction (the pthread QoS API on Apple platforms), and can be listed
//! with whether it is busy (U32 checks that all are idle after `suspend`).

use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

/// A thread's QoS class (Apple's classes; recorded only elsewhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Qos {
    UserInteractive,
    UserInitiated,
    Default,
    Utility,
    Background,
}

/// A live registered thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadInfo {
    /// Unique among the registry's threads, from 1.
    pub id: u64,
    pub name: String,
    /// The class the thread asked for.
    pub qos: Qos,
    /// The class the OS reports for the thread once it set it (Apple
    /// platforms); `None` where there is no QoS API.
    pub reported_qos: Option<Qos>,
    pub busy: bool,
    pub suspend_owner: Option<String>,
}

/// Where registered threads are listed. Clones share it.
#[derive(Clone, Debug, Default)]
pub struct ThreadRegistry {
    inner: Arc<Mutex<Registry>>,
}

#[derive(Debug, Default)]
struct Registry {
    next_id: u64,
    threads: BTreeMap<u64, Record>,
}

#[derive(Debug)]
struct Record {
    name: String,
    suspend_owner: Option<String>,
    qos: Qos,
    reported_qos: Option<Qos>,
    busy: Arc<AtomicBool>,
}

/// What a registered thread marks itself busy with.
#[derive(Debug)]
pub struct ThreadActivity {
    busy: Arc<AtomicBool>,
}

impl ThreadActivity {
    /// Marks the thread busy until the guard drops.
    pub fn busy(&self) -> BusyGuard<'_> {
        self.busy.store(true, Ordering::SeqCst);
        BusyGuard { activity: self }
    }
}

#[derive(Debug)]
pub struct BusyGuard<'a> {
    activity: &'a ThreadActivity,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.activity.busy.store(false, Ordering::SeqCst);
    }
}

impl ThreadRegistry {
    pub fn new() -> ThreadRegistry {
        ThreadRegistry::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a thread named `name` at `qos` running `work`. Returns once the
    /// thread has set its class and is listed; the thread leaves the list when
    /// `work` returns or panics.
    pub fn spawn<T: Send + 'static>(
        &self,
        name: impl Into<String>,
        qos: Qos,
        work: impl FnOnce(&ThreadActivity) -> T + Send + 'static,
    ) -> io::Result<JoinHandle<T>> {
        self.spawn_inner(name.into(), qos, None, work)
    }

    /// A core worker whose quiescence is owned by a participant, the writer
    /// drain, or the job registry. Unowned workers fail U32's enumeration.
    pub fn spawn_owned<T: Send + 'static>(
        &self,
        name: impl Into<String>,
        qos: Qos,
        owner: &str,
        work: impl FnOnce(&ThreadActivity) -> T + Send + 'static,
    ) -> io::Result<JoinHandle<T>> {
        self.spawn_inner(name.into(), qos, Some(owner.to_owned()), work)
    }

    fn spawn_inner<T: Send + 'static>(
        &self,
        name: String,
        qos: Qos,
        owner: Option<String>,
        work: impl FnOnce(&ThreadActivity) -> T + Send + 'static,
    ) -> io::Result<JoinHandle<T>> {
        let busy = Arc::new(AtomicBool::new(false));
        let id = self.list(Record {
            name: name.clone(),
            suspend_owner: owner,
            qos,
            reported_qos: None,
            busy: Arc::clone(&busy),
        });
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let registry = self.clone();
        let spawned = thread::Builder::new().name(name).spawn(move || {
            let _listed = Listed {
                registry: &registry,
                id,
            };
            let reported = apply_qos(qos);
            if let Some(record) = registry.lock().threads.get_mut(&id) {
                record.reported_qos = reported;
            }
            let _ = ready_tx.send(());
            work(&ThreadActivity { busy })
        });
        match spawned {
            Ok(handle) => {
                // The thread signals once listed with its class; if it died
                // first, its record is already gone.
                let _ = ready_rx.recv();
                Ok(handle)
            }
            Err(error) => {
                self.lock().threads.remove(&id);
                Err(error)
            }
        }
    }

    /// Lists the calling thread, which a runtime the core owns started (the
    /// sync runtime's tokio threads, KTD14) and so did not come from
    /// [`ThreadRegistry::spawn`], and sets its class as `spawn` does. It is
    /// listed idle until [`RegisteredThread::set_busy`], and leaves the list
    /// when the returned value drops.
    pub fn register_current(&self, name: impl Into<String>, qos: Qos) -> RegisteredThread {
        self.register_current_inner(name.into(), qos, None)
    }

    /// Registers a runtime thread with its owner atomically, before it can
    /// appear in a concurrent suspend enumeration.
    pub fn register_current_owned(&self, name: impl Into<String>, qos: Qos, owner: &str) -> RegisteredThread {
        self.register_current_inner(name.into(), qos, Some(owner.into()))
    }

    fn register_current_inner(&self, name: String, qos: Qos, owner: Option<String>) -> RegisteredThread {
        let busy = Arc::new(AtomicBool::new(false));
        let reported_qos = apply_qos(qos);
        let id = self.list(Record {
            name,
            suspend_owner: owner,
            qos,
            reported_qos,
            busy: Arc::clone(&busy),
        });
        RegisteredThread {
            registry: self.clone(),
            id,
            busy,
        }
    }

    pub(crate) fn utility_scope(&self) -> UtilityScope {
        let name = thread::current().name().map(str::to_owned);
        let mut state = self.lock();
        let prior = state
            .threads
            .iter_mut()
            .find(|(_, r)| Some(&r.name) == name.as_ref())
            .map(|(id, r)| {
                let old = r.qos;
                r.qos = Qos::Utility;
                r.reported_qos = apply_qos(Qos::Utility);
                (*id, old)
            });
        UtilityScope {
            registry: self.clone(),
            prior,
        }
    }

    /// Adds a thread to the list under a new id.
    fn list(&self, record: Record) -> u64 {
        let mut registry = self.lock();
        registry.next_id += 1;
        let id = registry.next_id;
        registry.threads.insert(id, record);
        id
    }

    /// Every live registered thread, by id.
    pub fn threads(&self) -> Vec<ThreadInfo> {
        self.lock()
            .threads
            .iter()
            .map(|(id, record)| ThreadInfo {
                id: *id,
                name: record.name.clone(),
                suspend_owner: record.suspend_owner.clone(),
                qos: record.qos,
                reported_qos: record.reported_qos,
                busy: record.busy.load(Ordering::SeqCst),
            })
            .collect()
    }

    /// The registered threads that are busy now.
    pub fn busy_threads(&self) -> Vec<ThreadInfo> {
        self.threads().into_iter().filter(|t| t.busy).collect()
    }
}

pub(crate) struct UtilityScope {
    registry: ThreadRegistry,
    prior: Option<(u64, Qos)>,
}
impl Drop for UtilityScope {
    fn drop(&mut self) {
        if let Some((id, qos)) = self.prior {
            let reported = apply_qos(qos);
            if let Some(record) = self.registry.lock().threads.get_mut(&id) {
                record.qos = qos;
                record.reported_qos = reported;
            }
        }
    }
}

/// A thread listed by [`ThreadRegistry::register_current`]. Keep it on that
/// thread and drop it before the thread ends: dropping it removes the thread
/// from the list.
#[derive(Debug)]
pub struct RegisteredThread {
    registry: ThreadRegistry,
    id: u64,
    busy: Arc<AtomicBool>,
}

impl RegisteredThread {
    pub fn set_suspend_owner(&self, owner: &str) {
        if let Some(record) = self.registry.lock().threads.get_mut(&self.id) {
            record.suspend_owner = Some(owner.into());
        }
    }

    /// Lists the thread busy or idle (a runtime's park and unpark hooks).
    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::SeqCst);
    }
}

impl Drop for RegisteredThread {
    fn drop(&mut self) {
        self.registry.lock().threads.remove(&self.id);
    }
}

/// Removes a thread from the list when it ends, normally or by a panic.
struct Listed<'a> {
    registry: &'a ThreadRegistry,
    id: u64,
}

impl Drop for Listed<'_> {
    fn drop(&mut self) {
        self.registry.lock().threads.remove(&self.id);
    }
}

/// Sets the calling thread's QoS class and returns what the OS reports.
#[cfg(target_vendor = "apple")]
fn apply_qos(qos: Qos) -> Option<Qos> {
    use libc::qos_class_t;
    let class = match qos {
        Qos::UserInteractive => qos_class_t::QOS_CLASS_USER_INTERACTIVE,
        Qos::UserInitiated => qos_class_t::QOS_CLASS_USER_INITIATED,
        Qos::Default => qos_class_t::QOS_CLASS_DEFAULT,
        Qos::Utility => qos_class_t::QOS_CLASS_UTILITY,
        Qos::Background => qos_class_t::QOS_CLASS_BACKGROUND,
    };
    // SAFETY: both calls act on the calling thread and write only to the
    // locals passed by pointer.
    unsafe {
        libc::pthread_set_qos_class_self_np(class, 0);
        let mut got = qos_class_t::QOS_CLASS_UNSPECIFIED;
        let mut relative = 0;
        if libc::pthread_get_qos_class_np(libc::pthread_self(), &mut got, &mut relative) != 0 {
            return None;
        }
        match got {
            qos_class_t::QOS_CLASS_USER_INTERACTIVE => Some(Qos::UserInteractive),
            qos_class_t::QOS_CLASS_USER_INITIATED => Some(Qos::UserInitiated),
            qos_class_t::QOS_CLASS_DEFAULT => Some(Qos::Default),
            qos_class_t::QOS_CLASS_UTILITY => Some(Qos::Utility),
            qos_class_t::QOS_CLASS_BACKGROUND => Some(Qos::Background),
            _ => None,
        }
    }
}

/// No QoS API: the class is recorded only.
#[cfg(not(target_vendor = "apple"))]
fn apply_qos(_qos: Qos) -> Option<Qos> {
    None
}
