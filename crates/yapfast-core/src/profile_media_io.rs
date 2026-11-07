//! Bounded, cancelable explicit-click profile downloads. The caller supplies
//! the existing allowlisted/cache-backed Discord fetch, never a new HTTP path.
use crate::original::{Completion, Request};
use crate::profile_media::{Loaded, MAX_CDN_SIZE};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::task::JoinHandle;

/// Same compressed-byte limit as attachment previews/viewing.
pub const MAX_BYTES: usize = 20 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(25);

pub(super) fn download_error(error: &anyhow::Error) -> &'static str {
    use yapfast_discord::api::{Refused, Stalled, TooLarge};
    if error.is::<TooLarge>() {
        "This profile original exceeds the viewer's safe download limit. Open the original to save it elsewhere."
    } else if error
        .downcast_ref::<Refused>()
        .is_some_and(|Refused(status)| status.as_u16() == 403)
    {
        "This profile image's link expired, or access was refused. Reopen the profile to refresh its assets."
    } else if error.is::<Stalled>() {
        "Discord stopped sending this profile image. Close it and try again."
    } else {
        "Harmony couldn't download this profile image. Close it and try again."
    }
}

#[derive(Default)]
pub struct Loader {
    current: Option<(u64, Request, JoinHandle<()>)>,
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.cancel_current();
    }
}
impl Loader {
    pub fn start<F, D>(&mut self, request: u64, cancellation: Request, fetch: F, done: D)
    where
        F: Future<Output = Result<Vec<u8>, &'static str>> + Send + 'static,
        D: FnOnce(Completion<Loaded>) + Send + 'static,
    {
        self.start_with_deadline(request, cancellation, fetch, done, DEADLINE);
    }
    fn start_with_deadline<F, D>(
        &mut self,
        request: u64,
        cancellation: Request,
        fetch: F,
        done: D,
        deadline: Duration,
    ) where
        F: Future<Output = Result<Vec<u8>, &'static str>> + Send + 'static,
        D: FnOnce(Completion<Loaded>) + Send + 'static,
    {
        self.start_with_decoder(
            request,
            cancellation,
            fetch,
            |data| crate::chat::decode_picture(data, MAX_CDN_SIZE, MAX_CDN_SIZE),
            done,
            deadline,
        );
    }

    /// The same production worker with an injectable decoder boundary. Tests
    /// can hold the real blocking closure without allocating large images.
    pub(crate) fn start_with_decoder<F, D, S>(
        &mut self,
        request: u64,
        cancellation: Request,
        fetch: F,
        decode: D,
        done: S,
        deadline: Duration,
    ) where
        F: Future<Output = Result<Vec<u8>, &'static str>> + Send + 'static,
        D: FnOnce(&[u8]) -> Option<crate::chat::Picture> + Send + 'static,
        S: FnOnce(Completion<Loaded>) + Send + 'static,
    {
        self.cancel_current();
        let active = cancellation.clone();
        let task = tokio::spawn(async move {
            let work = async {
                let permit = crate::original::admission()
                    .acquire_owned()
                    .await
                    .map_err(|_| "Image decoding stopped.")?;
                let data = fetch.await?;
                if data.len() > MAX_BYTES {
                    return Err("This profile image is too large to view safely.");
                }
                let bytes: Arc<[u8]> = data.into();
                let decoding = active.clone();
                tokio::task::spawn_blocking(move || {
                    // Already-running blocking work cannot be forcibly aborted.
                    // Move both bytes and lease through the blocking closure
                    // into its queued completion, not clonable UI state.
                    if decoding.is_cancelled() {
                        return Err("This image request was cancelled.");
                    }
                    let picture = decode(&bytes)
                        .map(Arc::new)
                        .ok_or("Harmony can't decode this image. Save it or open the original.");
                    Ok(Completion::admitted(
                        Loaded {
                            bytes: Some(bytes),
                            picture,
                        },
                        permit,
                    ))
                })
                .await
                .map_err(|_| "Image decoding stopped.")?
            };
            tokio::select! {
                biased;
                _ = active.cancelled() => {}
                result = tokio::time::timeout(deadline, work) => {
                    if !active.is_cancelled() {
                        let loaded = match result {
                            Ok(Ok(loaded)) => loaded,
                            Ok(Err(error)) => Completion::unadmitted(Loaded { bytes: None, picture: Err(error) }),
                            Err(_) => Completion::unadmitted(Loaded {
                                bytes: None,
                                picture: Err("This image took too long to load. Close it and try again."),
                            }),
                        };
                        done(loaded);
                    }
                }
            }
        });
        self.current = Some((request, cancellation, task));
    }
    pub fn cancel(&mut self, request: u64) {
        if self
            .current
            .as_ref()
            .is_some_and(|(current, _, _)| *current == request)
        {
            self.cancel_current();
        }
    }

    fn cancel_current(&mut self) {
        if let Some((_, cancellation, task)) = self.current.take() {
            cancellation.cancel();
            task.abort();
        }
    }
}

#[cfg(test)]
#[path = "profile_media_io_tests.rs"]
mod io_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn closing_drops_the_in_flight_fetch_without_delivering_a_picture() {
        let mut loader = Loader::default();
        let (started, wait_started) = tokio::sync::oneshot::channel();
        let (dropped, wait_dropped) = tokio::sync::oneshot::channel();
        let (finished, mut wait_finished) = tokio::sync::oneshot::channel();
        struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        loader.start(
            1,
            Request::new(),
            async move {
                let _drop = DropSignal(Some(dropped));
                let _ = started.send(());
                std::future::pending().await
            },
            move |loaded| {
                let _ = finished.send(loaded);
            },
        );
        wait_started.await.unwrap();
        loader.cancel(1);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), wait_dropped)
                .await
                .is_ok(),
            "cancel must drop the actual pending fetch"
        );
        assert!(wait_finished.try_recv().is_err());
    }
}
