//! Ownership of a single original-image request, separate from channel work.

use std::{
    future::Future,
    sync::{Arc, OnceLock},
};
use tokio::{
    sync::{Semaphore, watch},
    task::JoinHandle,
};

// Across channels, recently closed rooms, accounts and both providers. A
// cancelled spawn_blocking call cannot be interrupted: its permit must live
// INSIDE the closure, not in the abortable async awaiter. A completed result
// keeps the same permit while queued, until consumed or dropped.
static DECODING: OnceLock<Arc<Semaphore>> = OnceLock::new();

use crate::chat::Picture;

/// Identity and cancellation of one viewer request. Clones refer to that same
/// request, never just an attachment index (which can be revisited).
#[derive(Clone, Debug)]
pub struct Request(Arc<watch::Sender<bool>>);

impl PartialEq for Request {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Request {}

impl Request {
    pub(crate) fn new() -> Self {
        Self(Arc::new(watch::Sender::new(false)))
    }

    pub(crate) fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    async fn cancelled(&self) {
        let mut changed = self.0.subscribe();
        let _ = changed.wait_for(|cancelled| *cancelled).await;
    }
}

/// A completion owns its decoding slot until the application consumes it.
/// This prevents large completed originals accumulating in event queues.
#[derive(Debug)]
pub struct Decoded {
    picture: Arc<Picture>,
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl Decoded {
    pub(crate) fn into_picture(self) -> Arc<Picture> {
        self.picture
    }

    /// A tiny decoded fixture for core/adapter tests; production completions
    /// must come through Task so they carry admission ownership.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn fixture(picture: Arc<Picture>) -> Self {
        Self {
            picture,
            _permit: None,
        }
    }
}

/// The channel/room owns only this viewer task. Replacing or dropping it does
/// not abort previews, message history, sending or other channel work.
#[derive(Default)]
pub(crate) struct Task(Option<(Request, JoinHandle<()>)>);

impl Drop for Task {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Task {
    pub(crate) fn cancel(&mut self) {
        if let Some((request, task)) = self.0.take() {
            request.cancel();
            task.abort();
        }
    }

    /// Fetch on the runtime, decode on its blocking pool, then publish only if
    /// still wanted. The caller supplies the actual provider fetch/decoder/sink.
    pub(crate) fn start<F, D, S>(&mut self, request: Request, fetch: F, decode: D, sink: S)
    where
        F: Future<Output = Result<Vec<u8>, &'static str>> + Send + 'static,
        D: FnOnce(Vec<u8>) -> Option<Picture> + Send + 'static,
        S: FnOnce(Result<Decoded, &'static str>) + Send + 'static,
    {
        self.cancel();
        let active = request.clone();
        let task = tokio::spawn(async move {
            let work = async {
                // Admit before fetching, so abandoned originals cannot queue
                // many compressed downloads behind the single decoder either.
                let permit = DECODING
                    .get_or_init(|| Arc::new(Semaphore::new(1)))
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| "The image viewer has stopped.")?;
                let data = fetch.await?;
                let decoding = active.clone();
                tokio::task::spawn_blocking(move || {
                    if decoding.is_cancelled() {
                        return None;
                    }
                    decode(data).map(|picture| Decoded {
                        picture: Arc::new(picture),
                        _permit: Some(permit),
                    })
                })
                .await
                .ok()
                .flatten()
                .ok_or("Yapfast can't show this image. Save it to open it elsewhere.")
            };
            tokio::select! {
                biased;
                _ = active.cancelled() => {}
                picture = work => {
                    if !active.is_cancelled() {
                        sink(picture);
                    }
                }
            }
        });
        self.0 = Some((request, task));
    }
}
