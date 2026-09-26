use super::{WorkerConfig, process::Worker};
use labello_storage::prelabel::InferenceOwner;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Notify, Semaphore};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Owner {
    Inference(InferenceOwner),
    Inspection(u64),
}
struct Slot {
    worker: Option<Worker>,
    busy: bool,
    retired: bool,
    last_used: Instant,
}
pub(super) struct Pool {
    slots: Mutex<BTreeMap<Owner, Slot>>,
    changed: Notify,
    sequence: AtomicU64,
    closed: AtomicBool,
    pub config: WorkerConfig,
    pub budget: Arc<Semaphore>,
}

impl Pool {
    pub fn new(config: WorkerConfig) -> Arc<Self> {
        let pool = Arc::new(Self {
            slots: Mutex::new(BTreeMap::new()),
            changed: Notify::new(),
            sequence: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            budget: Arc::new(Semaphore::new(config.max_workers)),
            config,
        });
        let weak = Arc::downgrade(&pool);
        let period = Duration::from_secs(pool.config.idle_timeout_seconds.clamp(1, 30));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                let Some(pool) = weak.upgrade() else {
                    return;
                };
                pool.evict_expired();
            }
        });
        pool
    }

    fn evict_expired(&self) {
        self.slots.lock().unwrap().retain(|_, slot| {
            slot.busy
                || slot.last_used.elapsed() < Duration::from_secs(self.config.idle_timeout_seconds)
        });
        self.changed.notify_waiters();
    }

    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.slots.lock().unwrap().retain(|_, slot| {
            slot.retired = true;
            slot.busy
        });
        self.changed.notify_waiters();
    }

    pub fn inspection_owner(&self) -> Owner {
        Owner::Inspection(self.sequence.fetch_add(1, Ordering::Relaxed))
    }

    pub async fn acquire(
        self: &Arc<Self>,
        owner: Owner,
    ) -> Result<Lease, labello_storage::prelabel::PrelabelFailure> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let lease = {
                let mut slots = self.slots.lock().unwrap();
                if self.closed.load(Ordering::SeqCst) {
                    return Err(labello_storage::prelabel::PrelabelFailure::Busy);
                }
                if !slots.contains_key(&owner) && slots.len() == self.config.max_workers {
                    let oldest = slots
                        .iter()
                        .filter(|(_, slot)| !slot.busy)
                        .min_by_key(|(_, slot)| slot.last_used)
                        .map(|(key, _)| key.clone());
                    if let Some(oldest) = oldest {
                        slots.remove(&oldest);
                    }
                }
                if !slots.contains_key(&owner) && slots.len() < self.config.max_workers {
                    slots.insert(
                        owner.clone(),
                        Slot {
                            worker: None,
                            busy: false,
                            retired: false,
                            last_used: Instant::now(),
                        },
                    );
                }
                slots
                    .get_mut(&owner)
                    .filter(|slot| !slot.busy && !slot.retired)
                    .map(|slot| {
                        slot.busy = true;
                        Lease {
                            pool: self.clone(),
                            owner: owner.clone(),
                            worker: slot.worker.take(),
                            kept: false,
                        }
                    })
            };
            if let Some(lease) = lease {
                return Ok(lease);
            }
            changed.await;
        }
    }

    pub fn release(&self, owner: &Owner) {
        let mut slots = self.slots.lock().unwrap();
        if let Some(slot) = slots.get_mut(owner) {
            if slot.busy {
                slot.retired = true;
            } else {
                slots.remove(owner);
            }
        }
        self.changed.notify_waiters();
    }
}

pub(super) struct Lease {
    pool: Arc<Pool>,
    owner: Owner,
    pub worker: Option<Worker>,
    kept: bool,
}
impl Lease {
    pub fn keep(&mut self) {
        if let Some(slot) = self.pool.slots.lock().unwrap().get_mut(&self.owner)
            && !slot.retired
        {
            slot.worker = self.worker.take();
            slot.busy = false;
            slot.last_used = Instant::now();
            self.kept = true;
        }
        self.pool.changed.notify_waiters();
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.kept {
            self.worker.take();
            self.pool.slots.lock().unwrap().remove(&self.owner);
            self.pool.changed.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn expiry_removes_idle_slots_without_retiring_busy_owners() {
        let pool = Pool::new(WorkerConfig::default());
        let idle = pool.inspection_owner();
        let busy = pool.inspection_owner();
        let mut lease = pool.acquire(idle.clone()).await.unwrap();
        lease.keep();
        let _busy = pool.acquire(busy.clone()).await.unwrap();
        for slot in pool.slots.lock().unwrap().values_mut() {
            slot.last_used = Instant::now() - Duration::from_secs(121);
        }
        pool.evict_expired();
        let slots = pool.slots.lock().unwrap();
        assert!(!slots.contains_key(&idle));
        assert!(slots.contains_key(&busy));
    }
}
