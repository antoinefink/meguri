use meguri_sync::cloud::testing::IncomingStage;
use std::time::Duration;

// Positive boundary observation; release always precedes both scoped joins.
pub fn hold_native_authority_edge<T: Send, V: Send>(
    runtime: &meguri_sync::cloud::runtime::Runtime,
    edge: IncomingStage,
    transition: impl FnOnce() -> T + Send,
    validate: impl FnOnce() -> V + Send,
) -> (
    bool,
    Option<Result<V, std::sync::mpsc::RecvTimeoutError>>,
    std::thread::Result<T>,
    bool,
) {
    use std::sync::{Mutex, mpsc};
    struct Release(mpsc::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let resume = Mutex::new(resume);
    let (released, did_release) = mpsc::channel();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == edge {
            entered.send(()).unwrap();
            resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .expect("edge release");
            released.send(()).unwrap();
        }
    });
    let result = std::thread::scope(|scope| {
        let release = Release(release);
        let worker = scope.spawn(transition);
        let observed = reached.recv_timeout(Duration::from_secs(5)).is_ok();
        let (sent, received) = mpsc::channel();
        let validation = observed.then(|| {
            scope.spawn(move || {
                let _ = sent.send(validate());
            })
        });
        let before_release = validation
            .as_ref()
            .map(|_| received.recv_timeout(Duration::from_secs(5)));
        drop(release);
        let released = did_release.recv_timeout(Duration::from_secs(5)).is_ok();
        let result = worker.join();
        if let Some(validation) = validation {
            validation.join().unwrap();
        }
        (observed, before_release, result, released)
    });
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    result
}
