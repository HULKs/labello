use std::{cell::RefCell, collections::BTreeMap, future::Future, rc::Rc};

use futures::future::{AbortHandle, AbortRegistration, Abortable};
use labello_client::{ClientError, ClientResult};

#[derive(Default)]
pub(crate) struct ImageTransfers {
    transfers: Rc<RefCell<BTreeMap<u64, AbortHandle>>>,
}

impl ImageTransfers {
    pub fn transfer(&self, id: u64) -> ImageTransfer {
        let (handle, registration) = AbortHandle::new_pair();
        self.transfers.borrow_mut().insert(id, handle);
        ImageTransfer {
            id,
            registration: Some(registration),
            transfers: self.transfers.clone(),
        }
    }

    pub fn cancel_all(&self) {
        for (_, handle) in std::mem::take(&mut *self.transfers.borrow_mut()) {
            handle.abort();
        }
    }
}

pub(crate) struct ImageTransfer {
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
