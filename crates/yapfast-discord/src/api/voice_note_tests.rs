use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn voice_note_posts_one_multipart_message_with_ogg_audio_and_metadata() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut head = vec![];
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
            assert!(head.len() < 16_384);
        }
        let head = String::from_utf8(head).unwrap();
        let size: usize = head
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|v| v.parse().ok())
            })
            .unwrap();
        assert!(size < crate::voice_note::MAX_BYTES);
        let mut body = vec![0; size];
        stream.read_exact(&mut body).unwrap();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").unwrap();
        (head, body)
    });
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let api = Api {
        http: client.clone(),
        files: client,
        token: RwLock::new("test".into()),
        base,
        limits: Limits::default(),
        refusals: Refusals::default(),
        downloads: tokio::sync::Semaphore::new(1),
        stall: STALL,
    };
    let mut encoder = crate::voice_note::Builder::new().unwrap();
    encoder.push(&[0.25; 960]).unwrap();
    let note = encoder.finish().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(api.send_voice_note("123", &note, "123456"))
        .unwrap();
    let (head, bytes) = server.join().unwrap();
    assert!(head.starts_with("POST /channels/123/messages "));
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: multipart/form-data; boundary=")
    );
    let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|window| window == needle);
    assert!(contains(b"name=\"payload_json\"\r\n\r\n"));
    assert!(contains(
        note.message_body("123456").unwrap().to_string().as_bytes()
    ));
    assert!(contains(
        b"name=\"files[0]\"; filename=\"voice-message.ogg\"\r\nContent-Type: audio/ogg\r\n\r\n"
    ));
    assert!(contains(note.data()));
}
