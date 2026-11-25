//! Offline transport fixtures. No Api constructor, credentials or real IPC names.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct FixtureApplications(AtomicUsize);
impl Applications for FixtureApplications {
    async fn rpc_application(&self, id: &str) -> Result<Value> {
        self.0.fetch_add(1, Ordering::SeqCst);
        ensure!(id == "42", "unknown fixture app");
        Ok(json!({"name":"Owned RPC fixture"}))
    }
    async fn rpc_assets(&self, _: &str) -> Result<Value> {
        Ok(json!([]))
    }
    async fn rpc_external_assets(&self, _: &str, _: &[&str]) -> Result<Value> {
        Ok(json!([]))
    }
}
fn online() -> watch::Receiver<Arc<Snapshot>> {
    watch::channel(Arc::new(Snapshot {
        status: Status::Online,
        user_id: Some("fixture-user".into()),
        ..Default::default()
    }))
    .1
}
async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(client: &mut S) {
    write(client, 0, br#"{"v":1,"client_id":"42"}"#)
        .await
        .unwrap();
    let (opcode, bytes) = read(client).await.unwrap();
    assert_eq!(opcode, 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["evt"],
        "READY"
    );
}
#[tokio::test]
#[cfg(windows)]
async fn private_pipe_caps_active_clients_and_cancels_idle_and_pending_connections() {
    let name = format!(
        r"\\.\pipe\harmony-presence-cap-{}-{:032x}",
        std::process::id(),
        rand::random::<u128>()
    );
    let socket = Socket::bind_named(&name).unwrap();
    let lookup = Arc::new(FixtureApplications::default());
    let counted = lookup.clone();
    let (_snapshots, snapshot_rx) = watch::channel(Arc::new(Snapshot {
        status: Status::Online,
        user_id: Some("fixture-user".into()),
        ..Default::default()
    }));
    let (activity, receiver) = watch::channel(None);
    let task = tokio::spawn(async move {
        serve(socket, lookup, snapshot_rx, &activity).await;
    });
    let mut clients = Vec::new();
    for _ in 0..16 {
        let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .unwrap();
        handshake(&mut client).await;
        clients.push(client);
    }
    let mut pending = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(&name)
        .unwrap();
    write(&mut pending, 0, br#"{"v":1,"client_id":"42"}"#)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), read(&mut pending))
            .await
            .is_err()
    );
    assert_eq!(
        counted.0.load(Ordering::SeqCst),
        16,
        "the spare OS pipe must not spawn a seventeenth parser/API task"
    );
    assert!(
        tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .is_err(),
        "kernel instances are bounded too"
    );
    drop(receiver);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), read(&mut pending))
            .await
            .unwrap()
            .is_err()
    );
    drop(clients);
}

#[tokio::test]
#[cfg(windows)]
async fn private_pipe_uses_real_protocol_and_clears_on_disconnect_offline_and_cancel() {
    let name = format!(
        r"\\.\pipe\harmony-presence-rpc-{}-{:032x}",
        std::process::id(),
        rand::random::<u128>()
    );
    let socket = Socket::bind_named(&name).unwrap();
    let lookup = Arc::new(FixtureApplications::default());
    let (snapshots, snapshot_rx) = watch::channel(Arc::new(Snapshot {
        status: Status::Online,
        user_id: Some("fixture-user".into()),
        ..Default::default()
    }));
    let (activity, mut shown) = watch::channel(None);
    let task = tokio::spawn(async move {
        serve(socket, lookup, snapshot_rx, &activity).await;
    });
    let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(&name)
        .unwrap();
    handshake(&mut client).await;
    send(
        &mut client,
        json!({"cmd":"SET_VOICE_SETTINGS","nonce":"refused","args":{}}),
    )
    .await
    .unwrap();
    let (_, bytes) = read(&mut client).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["evt"],
        "ERROR"
    );
    let command = json!({"cmd":"SET_ACTIVITY","nonce":"owned","args":{"pid":1,"activity":{"name":"cannot inject app name","details":"Fixture only","type":0}}});
    send(&mut client, command.clone()).await.unwrap();
    read(&mut client).await.unwrap();
    shown.changed().await.unwrap();
    assert_eq!(
        shown.borrow_and_update().as_ref().unwrap()["name"],
        "Owned RPC fixture"
    );
    drop(client);
    tokio::time::timeout(Duration::from_secs(1), shown.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(shown.borrow_and_update().is_none());
    let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(&name)
        .unwrap();
    handshake(&mut client).await;
    send(&mut client, command).await.unwrap();
    read(&mut client).await.unwrap();
    shown.changed().await.unwrap();
    assert!(shown.borrow_and_update().is_some());
    snapshots.send_replace(Arc::new(Snapshot::default()));
    tokio::time::timeout(Duration::from_millis(100), shown.changed())
        .await
        .expect("going offline must revoke an idle client's activity")
        .unwrap();
    assert!(shown.borrow_and_update().is_none());
    drop(shown);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), read(&mut client))
            .await
            .unwrap()
            .is_err()
    );
    assert!(
        tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .is_err()
    );
}

#[tokio::test]
async fn partial_header_expires_instead_of_holding_a_client_slot() {
    let (mut client, mut server) = tokio::io::duplex(16);
    client.write_all(&[1]).await.unwrap();
    let result = tokio::time::timeout(WAIT + Duration::from_millis(250), read(&mut server)).await;
    assert!(
        result.is_ok(),
        "a started but incomplete frame must have a deadline"
    );
    assert!(result.unwrap().is_err());
}

#[tokio::test]
async fn ping_frames_cannot_bypass_the_connection_rate_bound() {
    let (mut client, server) = tokio::io::duplex(MAX_PACKET * 2);
    let lookup = Arc::new(FixtureApplications::default());
    let (updates, _incoming) = mpsc::channel(2);
    let task = tokio::spawn(connection(server, lookup.clone(), online(), updates, 1));
    handshake(&mut client).await;
    let mut refused = false;
    for _ in 0..65 {
        if write(&mut client, 3, b"ping").await.is_err() {
            refused = true;
            break;
        }
        if read(&mut client).await.is_err() {
            refused = true;
            break;
        }
    }
    assert!(
        refused,
        "a client must not have an unlimited PING/PONG CPU loop"
    );
    drop(client);
    assert!(task.await.unwrap().is_err());
    assert_eq!(lookup.0.load(Ordering::SeqCst), 1);
}
