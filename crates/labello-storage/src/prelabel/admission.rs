use super::{PrelabelFailure, Result};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

pub(super) struct Admission {
    slots: Arc<Semaphore>,
    interactive: AtomicUsize,
    changed: Notify,
}

struct Waiting<'a>(&'a Admission);
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.interactive.fetch_sub(1, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
}

impl Admission {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(limit)),
            interactive: AtomicUsize::new(0),
            changed: Notify::new(),
        }
    }

    pub(super) async fn interactive(&self) -> Result<OwnedSemaphorePermit> {
        let waiting = self.interactive.fetch_add(1, Ordering::SeqCst);
        let _waiting = Waiting(self);
        if waiting >= 64 {
            return Err(PrelabelFailure::Busy);
        }
        self.changed.notify_waiters();
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.slots.clone().acquire_owned(),
        )
        .await
        .map_err(|_| PrelabelFailure::Busy)?
        .map_err(|_| PrelabelFailure::Busy)
    }

    pub(super) async fn batch(&self) -> Result<OwnedSemaphorePermit> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.interactive.load(Ordering::SeqCst) != 0 {
                changed.await;
                continue;
            }
            tokio::select! {
                biased;
                _ = &mut changed => {},
                permit = self.slots.clone().acquire_owned() => {
                    let permit = permit.map_err(|_| PrelabelFailure::Busy)?;
                    if self.interactive.load(Ordering::SeqCst) == 0 { return Ok(permit); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        task::{Context, Waker},
    };

    #[tokio::test]
    async fn interactive_work_precedes_waiting_batch_and_cancellation_releases_priority() {
        let admission = Admission::new(1);
        let current = admission.batch().await.unwrap();
        let mut context = Context::from_waker(Waker::noop());
        let mut background = Box::pin(admission.batch());
        assert!(background.as_mut().poll(&mut context).is_pending());
        let mut foreground = Box::pin(admission.interactive());
        assert!(foreground.as_mut().poll(&mut context).is_pending());
        drop(current);
        assert!(background.as_mut().poll(&mut context).is_pending());
        let foreground_slot = foreground.await.unwrap();
        assert!(background.as_mut().poll(&mut context).is_pending());
        drop(foreground_slot);
        let background_slot = background.await.unwrap();
        let mut cancelled = Box::pin(admission.interactive());
        assert!(cancelled.as_mut().poll(&mut context).is_pending());
        drop(cancelled);
        drop(background_slot);
        assert!(admission.batch().await.is_ok());
        assert_eq!(admission.interactive.load(Ordering::SeqCst), 0);
    }
}
