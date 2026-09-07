use std::collections::BTreeMap;

use labello_domain::{AssignmentKind, AssignmentStatus, Timestamp, UserId};

use super::{DatasetRepository, StorageError, StorageResult, lease_expiration};

#[derive(Debug)]
pub(crate) struct CachedPresence {
    generation: u64,
    holders: BTreeMap<UserId, Timestamp>,
}

impl DatasetRepository {
    /// Read active annotation/review leases without claiming or renewing work.
    /// Expiration is checked even on cache hits. Clones share a single cold scan.
    pub async fn active_lease_holders(&self) -> StorageResult<BTreeMap<UserId, Timestamp>> {
        let mut cache = self.presence_cache.lock().await;
        let generation = self.assignment_availability_cache.generation();
        if let Some(value) = cache
            .as_ref()
            .filter(|value| value.generation == generation)
        {
            return Ok(unexpired(&value.holders, labello_domain::now()));
        }
        let index = self.load_images_index().await?;
        let mut images = index
            .images_by_hash
            .values()
            .map(|image| image.image_id.clone());
        let mut workers = tokio::task::JoinSet::new();
        let mut holders = BTreeMap::new();
        for image in images.by_ref().take(32) {
            let repo = self.clone();
            workers.spawn(async move { repo.load_image_state(&image).await });
        }
        while let Some(result) = workers.join_next().await {
            let state = result
                .map_err(|_| StorageError::BackgroundTask("presence scan failed".into()))??;
            for assignment in &state.assignments {
                if assignment.status != AssignmentStatus::Active
                    || !matches!(
                        assignment.kind,
                        AssignmentKind::Annotation | AssignmentKind::Review
                    )
                {
                    continue;
                }
                let expires = assignment
                    .expires_at
                    .unwrap_or_else(|| lease_expiration(assignment.updated_at));
                holders
                    .entry(assignment.assigned_to.clone())
                    .and_modify(|previous: &mut Timestamp| *previous = (*previous).max(expires))
                    .or_insert(expires);
            }
            if let Some(image) = images.next() {
                let repo = self.clone();
                workers.spawn(async move { repo.load_image_state(&image).await });
            }
        }
        let active = unexpired(&holders, labello_domain::now());
        // A concurrent commit invalidates this sample for subsequent requests.
        if generation == self.assignment_availability_cache.generation() {
            *cache = Some(CachedPresence {
                generation,
                holders,
            });
        }
        Ok(active)
    }
}

fn unexpired(holders: &BTreeMap<UserId, Timestamp>, now: Timestamp) -> BTreeMap<UserId, Timestamp> {
    holders
        .iter()
        .filter(|(_, expires)| **expires > now)
        .map(|(user, expires)| (user.clone(), *expires))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cached_presence_expires_without_a_write() {
        let temp = tempfile::tempdir().unwrap();
        let repo = DatasetRepository::new(temp.path());
        let now = labello_domain::now();
        *repo.presence_cache.lock().await = Some(CachedPresence {
            generation: repo.assignment_availability_cache.generation(),
            holders: BTreeMap::from([
                (UserId::from("expired"), now),
                (
                    UserId::from("active"),
                    now + std::time::Duration::from_secs(60),
                ),
            ]),
        });
        let active = repo.active_lease_holders().await.unwrap();
        assert_eq!(
            active.keys().collect::<Vec<_>>(),
            vec![&UserId::from("active")]
        );
    }
}
