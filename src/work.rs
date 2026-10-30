//! One worker and one pending request. New submissions replace pending work and
//! provide a generation token so expensive active work can stop at safe boundaries.
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
};

pub struct LatestWorker<T> {
    shared: Arc<Shared<T>>,
}
struct Shared<T> {
    pending: Mutex<(Option<(u64, T)>, bool, bool)>,
    ready: Condvar,
    generation: AtomicU64,
}
impl<T: Send + 'static> LatestWorker<T> {
    pub fn new(mut run: impl FnMut(u64, T, &AtomicU64) + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            pending: Mutex::new((None, false, false)),
            ready: Condvar::new(),
            generation: AtomicU64::new(0),
        });
        let worker = shared.clone();
        std::thread::spawn(move || {
            loop {
                let job = {
                    let mut pending = worker.pending.lock().unwrap_or_else(|e| e.into_inner());
                    while pending.0.is_none() && !pending.1 {
                        pending = worker
                            .ready
                            .wait(pending)
                            .unwrap_or_else(|e| e.into_inner());
                    }
                    if pending.1 {
                        return;
                    }
                    pending.2 = true;
                    pending.0.take().unwrap()
                };
                // A failed task must not leave flush() waiting forever or strand later requests.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(job.0, job.1, &worker.generation)
                }));
                if result.is_err() {
                    eprintln!("Background task panicked; worker remains available");
                }
                let mut pending = worker.pending.lock().unwrap_or_else(|e| e.into_inner());
                pending.2 = false;
                worker.ready.notify_all();
            }
        });
        Self { shared }
    }
    pub fn submit(&self, value: T) -> u64 {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let id = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        pending.0 = Some((id, value));
        self.shared.ready.notify_one();
        id
    }
    pub fn flush(&self) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while pending.0.is_some() || pending.2 {
            pending = self
                .shared
                .ready
                .wait(pending)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
    pub fn cancel(&self) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        pending.0 = None;
    }
}
impl<T> Drop for LatestWorker<T> {
    fn drop(&mut self) {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        pending.1 = true;
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        self.shared.ready.notify_one();
    }
}

/// At most one frame and one queued UI notification, even when rendering stalls.
pub struct LatestFrame<T> {
    slot: Mutex<(Option<T>, bool)>,
}
impl<T> Default for LatestFrame<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new((None, false)),
        }
    }
}
impl<T> LatestFrame<T> {
    pub fn publish(&self, value: T) -> bool {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        slot.0 = Some(value);
        let schedule = !slot.1;
        slot.1 = true;
        schedule
    }
    pub fn take(&self) -> Option<T> {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        slot.1 = false;
        slot.0.take()
    }
}
