use std::collections::VecDeque;

use labello_domain::{ImageRecord, PrelabelSuggestion};
use web_time::{Duration, Instant};

use crate::app::LoadedImage;

#[derive(Clone, Debug)]
pub struct QueuedImage {
    pub image: ImageRecord,
    pub prelabels: Vec<PrelabelSuggestion>,
}

#[derive(Clone, Debug)]
pub struct ImageQueue {
    queue_size: usize,
    loading: bool,
    failed_at: Option<Instant>,
    retry_delay: Duration,
    items: VecDeque<QueuedImage>,
    prepared: VecDeque<LoadedImage>,
}

impl ImageQueue {
    pub fn new(queue_size: usize) -> Self {
        Self {
            queue_size: queue_size.clamp(1, labello_domain::MAX_PRELOAD_QUEUE_SIZE),
            loading: false,
            failed_at: None,
            retry_delay: Duration::from_secs(1),
            items: VecDeque::new(),
            prepared: VecDeque::new(),
        }
    }

    pub fn queue_size(&self) -> usize {
        self.queue_size
    }

    pub fn set_queue_size(&mut self, queue_size: usize) -> Vec<labello_domain::Assignment> {
        self.queue_size = queue_size.clamp(1, labello_domain::MAX_PRELOAD_QUEUE_SIZE);
        let mut released = Vec::new();
        while self.len() > self.queue_size {
            if let Some(loaded) = self.prepared.pop_back() {
                released.push(loaded.assignment);
                continue;
            }
            self.items.pop_back();
        }
        released
    }

    pub(crate) fn retain_prepared(
        &mut self,
        mut keep: impl FnMut(&LoadedImage) -> bool,
    ) -> Vec<labello_domain::Assignment> {
        let mut released = Vec::new();
        self.prepared.retain(|loaded| {
            if keep(loaded) {
                true
            } else {
                released.push(loaded.assignment.clone());
                false
            }
        });
        released
    }

    pub(crate) fn prepared_assignment_ids(&self) -> Vec<labello_domain::AssignmentId> {
        self.prepared
            .iter()
            .map(|loaded| loaded.assignment.assignment_id.clone())
            .collect()
    }

    pub(crate) fn next_expiry(&self) -> Option<Duration> {
        self.prepared
            .iter()
            .filter_map(|loaded| loaded.prepared_until)
            .min()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    pub(crate) fn mark_failed(&mut self) {
        self.mark_failed_after(Duration::from_secs(1));
    }

    pub(crate) fn mark_failed_after(&mut self, delay: Duration) {
        self.failed_at = Some(Instant::now());
        self.retry_delay = delay;
    }

    pub(crate) fn clear_failure(&mut self) {
        self.failed_at = None;
        self.retry_delay = Duration::from_secs(1);
    }

    pub(crate) fn retry_due(&self) -> bool {
        self.failed_at
            .is_some_and(|failed| failed.elapsed() >= self.retry_delay)
    }

    pub(crate) fn retry_after(&self) -> Option<Duration> {
        self.failed_at
            .map(|failed| self.retry_delay.saturating_sub(failed.elapsed()))
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed_at.is_some()
    }

    pub fn len(&self) -> usize {
        self.items.len() + self.prepared.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.prepared.is_empty()
    }

    pub fn push_if_room(&mut self, image: QueuedImage) -> bool {
        if self.len() < self.queue_size {
            self.items.push_back(image);
            true
        } else {
            false
        }
    }

    pub fn pop_next(&mut self) -> Option<QueuedImage> {
        self.items.pop_front()
    }

    pub(crate) fn push_prepared(&mut self, image: LoadedImage) -> bool {
        if self.len() < self.queue_size {
            self.prepared.push_back(image);
            true
        } else {
            false
        }
    }

    pub(crate) fn pop_prepared(&mut self) -> Option<LoadedImage> {
        self.prepared.pop_front()
    }

    pub(crate) fn contains_assignment(&self, assignment: &labello_domain::Assignment) -> bool {
        self.prepared.iter().any(|loaded| {
            loaded.assignment.assignment_id == assignment.assignment_id
                && loaded.assignment.image_id == assignment.image_id
        })
    }

    pub(crate) fn remove_prepared_image(
        &mut self,
        image_id: &labello_domain::ImageId,
    ) -> Option<LoadedImage> {
        let index = self
            .prepared
            .iter()
            .position(|loaded| &loaded.assignment.image_id == image_id)?;
        self.prepared.remove(index)
    }

    pub(crate) fn drain_prepared_assignments(&mut self) -> Vec<labello_domain::Assignment> {
        self.prepared
            .drain(..)
            .map(|loaded| loaded.assignment)
            .collect()
    }

    pub(crate) fn prepared_image_ids(&self) -> Vec<labello_domain::ImageId> {
        self.prepared
            .iter()
            .map(|loaded| loaded.assignment.image_id.clone())
            .collect()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.prepared.clear();
        self.failed_at = None;
        self.retry_delay = Duration::from_secs(1);
    }
}

#[cfg(test)]
mod tests {
    use labello_domain::ImageId;

    use super::*;

    #[test]
    fn keeps_configured_size() {
        let mut queue = ImageQueue::new(2);
        assert!(queue.push_if_room(queued("a")));
        assert!(queue.push_if_room(queued("b")));
        assert!(!queue.push_if_room(queued("c")));
        queue.set_queue_size(1);
        assert_eq!(queue.len(), 1);
        assert_eq!(ImageQueue::new(99).queue_size(), 99);
        assert_eq!(
            ImageQueue::new(usize::MAX).queue_size(),
            labello_domain::MAX_PRELOAD_QUEUE_SIZE
        );
    }

    #[test]
    fn failed_refills_retry_after_a_short_delay() {
        let mut queue = ImageQueue::new(2);
        queue.mark_failed();
        assert!(queue.failed());
        assert!(!queue.retry_due());
        assert!(queue.retry_after().is_some());

        queue.failed_at = Some(Instant::now() - Duration::from_secs(1));
        assert!(queue.retry_due());
        queue.clear_failure();
        assert!(!queue.failed());
    }

    #[test]
    fn empty_refills_can_use_a_longer_retry_delay() {
        let mut queue = ImageQueue::new(2);
        queue.mark_failed_after(Duration::from_secs(15));
        assert!(!queue.retry_due());
        assert!(
            queue
                .retry_after()
                .is_some_and(|delay| delay > Duration::from_secs(14))
        );

        queue.failed_at = Some(Instant::now() - Duration::from_secs(15));
        assert!(queue.retry_due());
    }

    fn queued(id: &str) -> QueuedImage {
        QueuedImage {
            image: labello_domain::ImageRecord {
                image_id: ImageId::from(id),
                blake3: id.to_string(),
                canonical_path: format!("images/{id}.png"),
                known_paths: vec![],
                duplicate_paths: vec![],
                source_memberships: None,
                file_name: format!("{id}.png"),
                byte_size: 4,
                width: 10,
                height: 10,
                media_type: "image/png".to_string(),
            },
            prelabels: vec![],
        }
    }
}
