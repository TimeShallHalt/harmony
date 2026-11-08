//! Matrix and voice task ownership shared by native UI adapters.

use std::{path::PathBuf, sync::Arc};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::{
    chat::{self, Command, Event, Start},
    runtime::{BackgroundRuntime, BackgroundTask},
};

/// Keep this handle for the signed-in client's lifetime, independently of the
/// selected conversation. Drop it on the client thread to cancel network work.
/// Explicit logout is a command, so the session is revoked only after voice and
/// sync have stopped successfully.
pub struct Backend {
    commands: UnboundedSender<Command>,
    _actor: BackgroundTask<()>,
    // The actor must be cancelled before its runtime is released.
    _runtime: Arc<BackgroundRuntime>,
}

impl Backend {
    pub fn start(
        runtime: Arc<BackgroundRuntime>,
        root: PathBuf,
        start: Start,
    ) -> (Self, UnboundedReceiver<Event>) {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (sender, events) = mpsc::unbounded_channel();
        let actor = runtime.spawn(chat::run(root, start, receiver, sender));
        (
            Self {
                commands,
                _actor: actor,
                _runtime: runtime,
            },
            events,
        )
    }

    /// Anonymous, store-free command transport for native Adapter fixtures.
    /// Never runs chat::run, reads credentials, or starts service/device actors.
    #[cfg(feature = "test-fixtures")]
    pub fn anonymous_fixture(
        runtime: Arc<BackgroundRuntime>,
    ) -> (Self, UnboundedReceiver<Command>) {
        let (commands, receiver) = mpsc::unbounded_channel();
        let actor = runtime.spawn(std::future::pending());
        (
            Self {
                commands,
                _actor: actor,
                _runtime: runtime,
            },
            receiver,
        )
    }

    /// Return the command on failure so the application can clear pending state.
    pub fn send(&self, command: Command) -> Result<(), Command> {
        self.commands.send(command).map_err(|error| error.0)
    }
}

// Without Matrix, restoring depends on this machine's saved Discord login.
#[cfg(all(test, feature = "matrix"))]
mod tests {
    use super::*;

    #[test]
    fn failed_restore_closes_the_event_stream_and_rejects_commands() {
        let runtime = Arc::new(BackgroundRuntime::new().unwrap());
        let directory = tempfile::tempdir().unwrap();
        let (backend, mut events) =
            Backend::start(runtime.clone(), directory.path().into(), Start::Restore);
        runtime.block_on(async {
            let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
                .await
                .unwrap();
            assert!(matches!(event, Some(Event::Error(_))));
            assert!(events.recv().await.is_none());
        });
        assert!(matches!(
            backend.send(Command::Logout),
            Err(Command::Logout)
        ));
    }
}
