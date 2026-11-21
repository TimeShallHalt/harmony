//! Local Discord Rich Presence over Linux sockets or Windows named pipes.
//! Implements the local IPC transport documented at https://docs.discord.com/developers/topics/rpc.
//! Only local Rich Presence is exposed; account/voice control needs OAuth2
//! and is deliberately unsupported here.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(all(test, target_os = "linux"))]
use tokio::net::UnixListener;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, watch},
    task::JoinSet,
};

use crate::{
    api::Api,
    service::{Snapshot, Status},
};

#[cfg(target_os = "linux")]
mod unix;
#[cfg(target_os = "linux")]
use unix::{Socket, runtime_directory};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Socket;

/// The most image addresses a client's images are kept for: a music player
/// shows a new cover with each song.
const KEPT_IMAGES: usize = 32;
const MAX_CLIENTS: usize = 16;

/// A bounded sliding window; rejected traffic cannot grow this queue.
struct Rate {
    seen: std::collections::VecDeque<std::time::Instant>,
    capacity: usize,
}
impl Rate {
    fn new(capacity: usize) -> Self {
        Self {
            seen: Default::default(),
            capacity,
        }
    }
    fn allow(&mut self, now: std::time::Instant) -> bool {
        while self
            .seen
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= Duration::from_secs(60))
        {
            self.seen.pop_front();
        }
        if self.seen.len() >= self.capacity {
            return false;
        }
        self.seen.push_back(now);
        true
    }
}

pub(crate) fn start(
    api: Arc<Api>,
    snapshots: watch::Receiver<Arc<Snapshot>>,
) -> watch::Receiver<Option<Arc<Value>>> {
    let (activity, receiver) = watch::channel(None);
    tokio::spawn(async move {
        // Tests must opt in to avoid advertising their fixture account on
        // the desktop. An explicit directory also helps Wine/sandbox bridges.
        #[cfg(target_os = "linux")]
        let explicit = std::env::var_os("YAPFAST_DISCORD_RPC_DIR").is_some();
        #[cfg(windows)]
        let explicit = std::env::var_os("YAPFAST_DISCORD_RPC_PIPE").is_some();
        let disabled = std::env::var_os("YAPFAST_DISCORD_RPC").is_some_and(|v| v == "0")
            || (crate::client::test_server().is_some() && !explicit);
        if !disabled {
            match bind().await {
                Ok(socket) => {
                    tracing::info!(path = ?socket.path, "Discord RPC listening");
                    serve(socket, api, snapshots, &activity).await;
                    activity.send_replace(None);
                    return;
                }
                Err(error) => tracing::warn!(%error, "Discord RPC unavailable"),
            }
        }
        // Keep the sender alive without work until the service drops its
        // receiver. No retry timer or extra thread when RPC is disabled.
        activity.closed().await;
    });
    receiver
}

async fn bind() -> Result<Socket> {
    #[cfg(target_os = "linux")]
    {
        Socket::bind(
            &std::env::var_os("YAPFAST_DISCORD_RPC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(runtime_directory),
        )
        .await
    }
    #[cfg(windows)]
    {
        if let Some(name) = std::env::var_os("YAPFAST_DISCORD_RPC_PIPE") {
            return Socket::bind_named(
                &name
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("invalid pipe name"))?,
            )
            .map_err(Into::into);
        }
        for slot in 0..10 {
            if let Ok(socket) = Socket::bind_named(&format!(r"\\.\pipe\discord-ipc-{slot}")) {
                return Ok(socket);
            }
        }
        bail!("no free Discord Rich Presence pipe")
    }
}

// The production API remains the sole lookup implementation. Fixtures inject
// only this I/O boundary, never protocol/parser or activity output.
trait Applications: Send + Sync + 'static {
    fn rpc_application(&self, id: &str) -> impl std::future::Future<Output = Result<Value>> + Send;
    fn rpc_assets(&self, id: &str) -> impl std::future::Future<Output = Result<Value>> + Send;
    fn rpc_external_assets(
        &self,
        id: &str,
        urls: &[&str],
    ) -> impl std::future::Future<Output = Result<Value>> + Send;
}
impl Applications for Api {
    async fn rpc_application(&self, id: &str) -> Result<Value> {
        Api::rpc_application(self, id).await
    }
    async fn rpc_assets(&self, id: &str) -> Result<Value> {
        Api::rpc_assets(self, id).await
    }
    async fn rpc_external_assets(&self, id: &str, urls: &[&str]) -> Result<Value> {
        Api::rpc_external_assets(self, id, urls).await
    }
}

async fn serve<A: Applications>(
    mut socket: Socket,
    api: Arc<A>,
    mut snapshots: watch::Receiver<Arc<Snapshot>>,
    activity: &watch::Sender<Option<Arc<Value>>>,
) {
    let (updates, mut incoming) = mpsc::channel(MAX_CLIENTS);
    let mut clients = JoinSet::new();
    let mut connected = HashSet::new();
    let mut next = 0_u64;
    let mut active = Vec::<(u64, Arc<Value>)>::new();
    let mut connections = Rate::new(32);
    let mut account = snapshots.borrow().user_id.clone();
    loop {
        let removed = tokio::select! {
            biased;
            () = activity.closed() => break,
            result = snapshots.changed() => {
                if result.is_err() { break; }
                let snapshot = snapshots.borrow_and_update();
                if snapshot.status != Status::Online || snapshot.user_id != account {
                    clients.abort_all();
                    connected.clear();
                    active.clear();
                }
                account = snapshot.user_id.clone();
                None
            }
            accepted = socket.accept(), if clients.len() < MAX_CLIENTS => {
                let Ok(stream) = accepted else {
                    // Out of descriptors, say: wait rather than end RPC for good.
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                };
                if !connections.allow(std::time::Instant::now()) {
                    drop(stream);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                next += 1;
                let id = next;
                connected.insert(id);
                let api = api.clone();
                let snapshots = snapshots.clone();
                let updates = updates.clone();
                clients.spawn(async move {
                    if let Err(error) = connection(stream, api, snapshots, updates, id).await {
                        tracing::debug!(%error, "Discord RPC client ended");
                    }
                    id
                });
                continue;
            }
            Some((id, value)) = incoming.recv() => {
                // A client may finish between polling this queue and polling
                // JoinSet. Its queued final update must not resurrect a game.
                if !connected.contains(&id) {
                    continue;
                }
                active.retain(|(other, _)| *other != id);
                if let Some(value) = value {
                    active.push((id, value));
                }
                None
            }
            Some(done) = clients.join_next(), if !clients.is_empty() => done.ok(),
        };
        if let Some(id) = removed {
            connected.remove(&id);
            active.retain(|(other, _)| *other != id);
        }
        activity.send_if_modified(|shown| {
            let next = active.last().map(|(_, value)| value);
            if shown.as_ref() == next {
                false
            } else {
                *shown = next.cloned();
                true
            }
        });
    }
    activity.send_replace(None);
    // Dropping JoinSet aborts clients; platform handles/inodes belong only to us.
}

include!("rpc/protocol.rs");

// Transport trust is SAME USER (Unix peer UID / protected Windows SID DACL).
// Games share activities while the account allows game sharing.
// Neither transport authenticates a kernel peer PID here; args.pid is untrusted
// protocol data, not proof of a game executable, process ownership or lifetime.
async fn connection<S: AsyncRead + AsyncWrite + Unpin, A: Applications>(
    mut stream: S,
    api: Arc<A>,
    snapshots: watch::Receiver<Arc<Snapshot>>,
    updates: mpsc::Sender<(u64, Option<Arc<Value>>)>,
    id: u64,
) -> Result<()> {
    let (opcode, payload) = tokio::time::timeout(WAIT, read(&mut stream)).await??;
    let handshake: Value = serde_json::from_slice(&payload)?;
    let client = handshake["client_id"].as_str().unwrap_or_default();
    if opcode != 0 || handshake["v"] != 1 || !snowflake(client) {
        write(
            &mut stream,
            2,
            br#"{"code":4000,"message":"Invalid RPC handshake"}"#,
        )
        .await?;
        return Ok(());
    }
    let client = client.to_owned();
    // A connected, idle game must not retain its handshake JSON/packet buffer.
    drop(handshake);
    drop(payload);
    // A game that retries while Discord is offline must not cost an API call each time.
    ensure!(
        snapshots.borrow().status == Status::Online,
        "Discord is not connected"
    );
    let application = tokio::time::timeout(WAIT, api.rpc_application(&client)).await??;
    let name = application["name"]
        .as_str()
        .context("RPC application has no name")?
        .to_owned();
    drop(application);
    let user = {
        let snapshot = snapshots.borrow();
        ensure!(
            snapshot.status == Status::Online,
            "Discord is not connected"
        );
        json!({"id": snapshot.user_id, "username": snapshot.account.as_ref().map(|a| a.name.as_str()),
            "discriminator": "0", "avatar": null, "bot": false})
    };
    send(&mut stream, json!({"cmd":"DISPATCH", "evt":"READY", "nonce":null,
        "data":{"v":1,"config":{"cdn_host":"cdn.discordapp.com","api_endpoint":"//discord.com/api","environment":"production"},"user":user}})).await?;
    let mut assets = None;
    let mut images = Assets::new();
    let mut frames = Rate::new(60);
    loop {
        let (opcode, payload) = read(&mut stream).await?;
        ensure!(
            frames.allow(std::time::Instant::now()),
            "RPC frame rate exceeded"
        );
        match opcode {
            2 => return Ok(()),
            3 => {
                write(&mut stream, 4, &payload).await?;
                continue;
            }
            4 => continue,
            1 => {}
            _ => bail!("invalid RPC opcode"),
        }
        let packet: Value = serde_json::from_slice(&payload)?;
        drop(payload);
        let cmd = packet["cmd"].as_str().unwrap_or_default();
        let nonce = &packet["nonce"];
        if cmd != "SET_ACTIVITY" {
            send(
                &mut stream,
                json!({"cmd":cmd,"nonce":nonce,"evt":"ERROR",
                "data":{"code":4002,"message":"Only SET_ACTIVITY is supported"}}),
            )
            .await?;
            continue;
        }
        let result = async {
            let args = &packet["args"];
            ensure!(
                args["pid"].as_u64().is_some_and(|pid| pid > 0),
                "pid must be a positive integer"
            );
            // The Discord RPC SDK omits activity when clearing. Some clients send null.
            if args["activity"].is_null() {
                return Ok(None);
            }
            let pictures = &args["activity"]["assets"];
            let (addresses, keys): (Vec<&str>, Vec<&str>) = ["large_image", "small_image"]
                .iter()
                .filter_map(|key| pictures[key].as_str())
                .partition(|image| web_address(image, 512));
            let addresses: Vec<&str> = addresses
                .into_iter()
                .filter(|address| !images.contains_key(*address))
                .collect();
            if assets.is_none() && !keys.is_empty() {
                let response = tokio::time::timeout(WAIT, api.rpc_assets(&client)).await??;
                assets = Some(asset_lookup(&response)?);
            }
            if !addresses.is_empty() {
                // As the desktop client does, Discord's media proxy takes them.
                let response =
                    tokio::time::timeout(WAIT, api.rpc_external_assets(&client, &addresses))
                        .await??;
                if images.len() + addresses.len() > KEPT_IMAGES {
                    images.clear();
                }
                images.extend(external_lookup(&response)?);
            }
            normalize(&args["activity"], &client, &name, assets.as_ref(), &images)
                .map(|value| Some(Arc::new(value)))
        }
        .await;
        match result {
            Ok(value) => {
                // Bound queued updates even when games publish very frequently.
                updates.send((id, value.clone())).await?;
                send(
                    &mut stream,
                    json!({"cmd":cmd,"nonce":nonce,"evt":null,"data":value}),
                )
                .await?;
            }
            Err(error) => {
                send(
                    &mut stream,
                    json!({"cmd":cmd,"nonce":nonce,"evt":"ERROR",
                    "data":{"code":4000,"message":error.to_string()}}),
                )
                .await?;
            }
        }
    }
}

#[cfg(test)]
mod transport_tests;

#[cfg(test)]
#[path = "rpc/protocol_tests.rs"]
mod tests;
