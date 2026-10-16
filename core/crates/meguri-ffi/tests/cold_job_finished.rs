//! A cold process (this binary holds one test, so nothing else touched the
//! runtime first): a job that ends before Swift attaches its listener still
//! reaches it (`engine/change-stream` S3: events published before the
//! listener wait in the dispatcher's subscription).

use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use meguri_ffi::events::set_event_listener;
use meguri_ffi::{CoreEvent, CoreEventKind, CoreEventListener, JobOutcome, jobs};

struct Forward(Mutex<mpsc::Sender<CoreEvent>>);

impl CoreEventListener for Forward {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

#[test]
fn a_job_that_ends_before_the_listener_is_attached_is_still_delivered() {
    let (done_tx, done_rx) = mpsc::channel();
    let handle = jobs::start("cold", 0, move |_| {
        done_tx.send(()).unwrap();
        Ok(())
    })
    .unwrap();
    done_rx.recv_timeout(Duration::from_secs(10)).expect("the job ran");
    // Long enough for the registry to publish its JobFinished.
    thread::sleep(Duration::from_millis(200));

    let (tx, rx) = mpsc::channel();
    set_event_listener(Arc::new(Forward(Mutex::new(tx)))).unwrap();
    let event = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the JobFinished published before the listener was lost");
    assert_eq!(
        event.kind,
        CoreEventKind::JobFinished {
            id: handle.id,
            outcome: JobOutcome::Succeeded
        }
    );
}
