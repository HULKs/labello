use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    future::Future,
    rc::Rc,
};

use futures::future::{AbortHandle, AbortRegistration, Abortable};
use labello_client::{ClientError, ClientResult};
use labello_domain::{DatasetId, ImageId};

const PREVIEW_CACHE_BYTES: usize = 32 * 1024 * 1024;

struct CachedPreview {
    dataset: DatasetId,
    image: ImageId,
    pixels: eframe::egui::ColorImage,
}

#[derive(Clone, Default)]
pub(crate) struct PreviewCache(Rc<RefCell<VecDeque<CachedPreview>>>);

impl PreviewCache {
    pub fn get(&self, dataset: &DatasetId, image: &ImageId) -> Option<eframe::egui::ColorImage> {
        let mut entries = self.0.borrow_mut();
        let index = entries
            .iter()
            .position(|entry| &entry.dataset == dataset && &entry.image == image)?;
        let entry = entries.remove(index)?;
        let pixels = entry.pixels.clone();
        entries.push_back(entry);
        Some(pixels)
    }

    pub fn insert(&self, dataset: DatasetId, image: ImageId, pixels: eframe::egui::ColorImage) {
        let mut entries = self.0.borrow_mut();
        entries.retain(|entry| entry.dataset != dataset || entry.image != image);
        entries.push_back(CachedPreview {
            dataset,
            image,
            pixels,
        });
        while entries.len() > 8
            || entries
                .iter()
                .map(|entry| entry.pixels.pixels.len() * size_of::<eframe::egui::Color32>())
                .sum::<usize>()
                > PREVIEW_CACHE_BYTES
        {
            entries.pop_front();
        }
    }
}

#[derive(Default)]
pub(crate) struct ImageTransfers {
    transfers: Rc<RefCell<BTreeMap<u64, AbortHandle>>>,
    previews: PreviewCache,
}

impl ImageTransfers {
    pub fn transfer(&self, id: u64) -> ImageTransfer {
        let (handle, registration) = AbortHandle::new_pair();
        self.transfers.borrow_mut().insert(id, handle);
        ImageTransfer {
            id,
            registration: Some(registration),
            transfers: self.transfers.clone(),
            previews: self.previews.clone(),
        }
    }

    pub fn cancel(&self, id: u64) {
        if let Some(handle) = self.transfers.borrow_mut().remove(&id) {
            handle.abort();
        }
    }

    pub fn cancel_all(&self) {
        for (_, handle) in std::mem::take(&mut *self.transfers.borrow_mut()) {
            handle.abort();
        }
    }

    pub fn clear_previews(&mut self) {
        // Outstanding work retains the old cache and cannot repopulate this scope.
        self.previews = PreviewCache::default();
    }
}

pub(crate) struct ImageTransfer {
    pub previews: PreviewCache,
    id: u64,
    registration: Option<AbortRegistration>,
    transfers: Rc<RefCell<BTreeMap<u64, AbortHandle>>>,
}
impl ImageTransfer {
    pub async fn run<T>(
        mut self,
        future: impl Future<Output = ClientResult<T>>,
    ) -> ClientResult<T> {
        Abortable::new(
            future,
            self.registration.take().expect("one image transfer"),
        )
        .await
        .map_err(|_| ClientError::Api {
            status: 0,
            message: "image request superseded".into(),
        })?
    }
}
impl Drop for ImageTransfer {
    fn drop(&mut self) {
        self.transfers.borrow_mut().remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_cache_is_bounded_scoped_and_keeps_recent_images() {
        let mut transfers = ImageTransfers::default();
        let old_scope = transfers.transfer(1).previews.clone();
        let pixels =
            || eframe::egui::ColorImage::filled([1024, 1024], eframe::egui::Color32::WHITE);
        for index in 0..8 {
            old_scope.insert("dataset".into(), format!("image-{index}").into(), pixels());
        }
        assert!(
            old_scope
                .get(&"dataset".into(), &"image-0".into())
                .is_some()
        );
        old_scope.insert("dataset".into(), "image-8".into(), pixels());
        assert!(
            old_scope
                .get(&"dataset".into(), &"image-1".into())
                .is_none()
        );
        assert!(old_scope.get(&"other".into(), &"image-0".into()).is_none());
        assert!(
            old_scope
                .get(&"dataset".into(), &"image-0".into())
                .is_some()
        );
        transfers.clear_previews();
        old_scope.insert("dataset".into(), "late".into(), pixels());
        assert!(
            transfers
                .previews
                .get(&"dataset".into(), &"late".into())
                .is_none()
        );
        assert!(
            transfers
                .previews
                .get(&"dataset".into(), &"image-0".into())
                .is_none()
        );
        old_scope.insert(
            "dataset".into(),
            "oversized".into(),
            eframe::egui::ColorImage::filled([4096, 4096], eframe::egui::Color32::WHITE),
        );
        assert!(old_scope.0.borrow().is_empty());
    }

    #[test]
    fn cancellation_drops_image_transfers_and_clears_the_registry() {
        let transfers = ImageTransfers::default();
        let transfer = transfers.transfer(1);
        transfers.cancel_all();
        let result: ClientResult<()> =
            futures::executor::block_on(transfer.run(futures::future::pending()));
        assert!(result.is_err());
        assert!(transfers.transfers.borrow().is_empty());
    }
}
