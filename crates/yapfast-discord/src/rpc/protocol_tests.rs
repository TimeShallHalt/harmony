use super::*;
use crate::own_activity::merge;
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;

#[cfg(target_os = "linux")]
struct Directory(PathBuf);
#[cfg(target_os = "linux")]
impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "yapfast-rpc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
#[cfg(target_os = "linux")]
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn converts_sdk_fields_and_uses_the_handshakes_application() {
    let assets = asset_lookup(&json!([{"name":"cover","id":"1234"}])).unwrap();
    let shown = normalize(
        &json!({"name":"Spoof","application_id":"999","state":"In game",
            "timestamps":{"start":1700000000_u64,"end":1700000123000_u64},
            "assets":{"large_image":"cover","small_image":"5678","large_text":"Hover"},
            "buttons":[{"label":"Website","url":"https://example.com"}],
            "metadata":{"injected":true},"id":"spoof"}),
        "42",
        "Game",
        Some(&assets),
        &Assets::new(),
    )
    .unwrap();
    assert_eq!(shown["type"], 0);
    assert_eq!(shown["name"], "Game");
    assert_eq!(shown["application_id"], "42");
    assert_eq!(
        shown["timestamps"],
        json!({"start":1700000000000_u64,"end":1700000123000_u64})
    );
    assert_eq!(shown["assets"]["large_image"], "1234");
    assert_eq!(shown["assets"]["small_image"], "5678");
    assert_eq!(shown["buttons"], json!(["Website"]));
    assert_eq!(
        shown["metadata"],
        json!({"button_urls":["https://example.com"]})
    );
    assert!(shown.get("id").is_none());
}

#[test]
fn shows_images_given_by_address_through_the_media_proxy() {
    let images = external_lookup(&json!([{"url":"https://example.com/cover.png",
            "external_asset_path":"external/abc/https/example.com/cover.png"}]))
    .unwrap();
    let activity = json!({"assets":{"large_image":"https://example.com/cover.png"}});
    let shown = normalize(&activity, "42", "Game", None, &images).unwrap();
    assert_eq!(
        shown["assets"]["large_image"],
        "mp:external/abc/https/example.com/cover.png"
    );
    let other = json!({"assets":{"large_image":"https://example.com/other.png"}});
    assert!(normalize(&other, "42", "Game", None, &images).is_err());
}

#[test]
fn rejects_invalid_activity_without_losing_the_previous_one() {
    for value in [
        json!([]),
        json!({"type":4}),
        json!({"type":"0"}),
        json!({"state":42}),
        json!({"details":"a".repeat(129)}),
        json!({"timestamps":{"start":-1}}),
        json!({"assets":{"large_image":"unknown"}}),
        json!({"buttons":[{"label":"Click","url":"javascript:alert(1)"}]}),
    ] {
        assert!(
            normalize(&value, "42", "Game", None, &Assets::new()).is_err(),
            "{value}"
        );
    }
    for kind in [0, 2, 3, 5] {
        assert!(normalize(&json!({"type":kind}), "42", "Game", None, &Assets::new()).is_ok());
    }
}

#[test]
fn merging_keeps_custom_status_and_availability_and_replaces_stale_games() {
    let custom = json!({"type":4,"name":"Custom Status","state":"Tea"});
    let mut presence = json!({"status":"dnd","since":123,"afk":true,
        "activities":[custom,{"type":0,"name":"Old game"}]});
    let song = json!({"type":2,"name":"Spotify"});
    merge(
        &mut presence,
        Some(&json!({"type":0,"name":"Game"})),
        Some(&song),
    );
    assert_eq!(presence["activities"].as_array().unwrap().len(), 3);
    assert_eq!(presence["activities"][1]["name"], "Game");
    assert_eq!(presence["activities"][2], song, "the music goes last");
    merge(&mut presence, None, None);
    assert_eq!(
        presence,
        json!({"status":"dnd","since":123,"afk":true,"activities":[custom]})
    );
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn leaves_live_sockets_alone_and_reclaims_stale_ones() {
    let directory = Directory::new();
    let existing = UnixListener::bind(directory.0.join("discord-ipc-0")).unwrap();
    let first = Socket::bind(&directory.0).await.unwrap();
    assert!(first.path.ends_with("discord-ipc-1"));
    let second = Socket::bind(&directory.0).await.unwrap();
    assert!(second.path.ends_with("discord-ipc-2"));
    assert_eq!(
        std::fs::metadata(&first.path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(first);
    assert!(!directory.0.join("discord-ipc-1").exists());
    drop(existing);
    let reclaimed = Socket::bind(&directory.0).await.unwrap();
    assert!(reclaimed.path.ends_with("discord-ipc-0"));
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn does_not_remove_a_replacement_or_follow_a_lock_symlink() {
    let directory = Directory::new();
    let target = directory.0.join("keep");
    std::fs::write(&target, b"keep me").unwrap();
    std::os::unix::fs::symlink(&target, directory.0.join("yapfast-discord-ipc-0.lock")).unwrap();
    let socket = Socket::bind(&directory.0).await.unwrap();
    assert!(socket.path.ends_with("discord-ipc-1"));
    std::fs::remove_file(&socket.path).unwrap();
    std::fs::write(&socket.path, b"replacement").unwrap();
    let path = socket.path.clone();
    drop(socket);
    assert_eq!(std::fs::read(path).unwrap(), b"replacement");
    assert_eq!(std::fs::read(target).unwrap(), b"keep me");
}

#[tokio::test]
async fn outgoing_packets_reject_oversize_before_writing_a_header() {
    let (mut server, mut client) = tokio::io::duplex(MAX_PACKET * 2);
    assert!(
        write(&mut server, 1, &vec![b'x'; MAX_PACKET + 1])
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), client.read_u8())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn framing_handles_partial_reads_and_rejects_oversize_before_allocating() {
    let (mut client, mut server) = tokio::io::duplex(32);
    let writer = tokio::spawn(async move {
        for byte in [1, 0, 0, 0, 2, 0, 0, 0, b'{', b'}'] {
            client.write_all(&[byte]).await.unwrap();
            tokio::task::yield_now().await;
        }
        client.write_u32_le(1).await.unwrap();
        client.write_u32_le(MAX_PACKET as u32 + 1).await.unwrap();
    });
    assert_eq!(read(&mut server).await.unwrap(), (1, b"{}".to_vec()));
    assert!(
        read(&mut server)
            .await
            .unwrap_err()
            .to_string()
            .contains("too large")
    );
    writer.await.unwrap();
}
