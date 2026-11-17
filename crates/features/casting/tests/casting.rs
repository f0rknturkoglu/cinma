#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};

use cinma_blobstore::MemorySource;
use cinma_casting::chromecast::{decode_cast_message, encode_cast_message};
use cinma_casting::server::{CastServerHandle, MediaPayload};
use cinma_casting::{ActiveCastSession, CastDevice, CastKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn castv2_protobuf_framing_roundtrips() {
    let encoded = encode_cast_message(
        "sender-0",
        "receiver-0",
        "urn:x-cast:com.google.cast.media",
        r#"{"type":"PLAY","requestId":1}"#,
    );
    let decoded = decode_cast_message(&encoded);
    assert_eq!(decoded.namespace, "urn:x-cast:com.google.cast.media");
    assert_eq!(decoded.payload_json.unwrap()["type"], "PLAY");
}

#[tokio::test]
async fn lan_http_server_serves_byte_range_and_drives_dlna_soap_session() {
    let data: Vec<u8> = (0..128 * 1024).map(|i| (i % 251) as u8).collect();
    let source: Arc<dyn cinma_blobstore::ByteSource> =
        Arc::new(MemorySource::new(data.clone(), data.len() as u64));

    let handle = CastServerHandle::start(
        MediaPayload::Source {
            source: Arc::clone(&source),
            mime: "video/x-matroska".into(),
        },
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    )
    .await
    .unwrap();

    // Invalid token -> 404
    let client = reqwest::Client::new();
    let bad = client
        .get(format!(
            "http://127.0.0.1:{}/cast/wrong",
            handle.bind_addr.port()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status().as_u16(), 404);

    // Partial HTTP Range 206
    let partial = client
        .get(&handle.local_url)
        .header("Range", "bytes=1024-2047")
        .send()
        .await
        .unwrap();
    assert_eq!(partial.status().as_u16(), 206);
    let bytes = partial.bytes().await.unwrap();
    assert_eq!(bytes.as_ref(), &data[1024..2048]);
    handle.stop().await;

    // Mock DLNA renderer + ActiveCastSession lifecycle
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dlna_addr = listener.local_addr().unwrap();
    let seen_actions = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_ref = Arc::clone(&seen_actions);
    let srv = tokio::spawn(async move {
        for _ in 0..4 {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            for act in ["SetAVTransportURI", "Play", "Pause", "Stop"] {
                if req.contains(act) {
                    seen_ref.lock().unwrap().push(act.to_owned());
                }
            }
            let ok = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            sock.write_all(ok.as_bytes()).await.unwrap();
        }
    });

    let device = CastDevice {
        id: "dlna:mock".into(),
        kind: CastKind::Dlna,
        name: "Living Room TV".into(),
        host: "127.0.0.1".into(),
        port: dlna_addr.port(),
        control_url: Some(format!("http://{dlna_addr}/AVTransport/control")),
    };
    let session = ActiveCastSession::start(
        device,
        MediaPayload::Source {
            source,
            mime: "video/x-matroska".into(),
        },
        "Example Movie",
    )
    .await
    .unwrap();
    let t_pause = std::time::Instant::now();
    session.pause().await.unwrap();
    assert!(t_pause.elapsed() < std::time::Duration::from_millis(500));

    let t_stop = std::time::Instant::now();
    session.stop().await.unwrap();
    assert!(t_stop.elapsed() < std::time::Duration::from_millis(500));
    srv.await.unwrap();

    assert_eq!(
        *seen_actions.lock().unwrap(),
        vec!["SetAVTransportURI", "Play", "Pause", "Stop"]
    );
}

#[tokio::test]
async fn cast_stop_does_not_hang_when_renderer_stops_responding() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dlna_addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        // First two requests (SetAVTransportURI + Play) succeed fast; Stop hangs forever.
        for i in 0..3 {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await.unwrap();
            if i < 2 {
                let ok = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                sock.write_all(ok.as_bytes()).await.unwrap();
            } else {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        }
    });

    let source: Arc<dyn cinma_blobstore::ByteSource> =
        Arc::new(MemorySource::new(vec![1; 4096], 4096));
    let device = CastDevice {
        id: "dlna:stalled".into(),
        kind: CastKind::Dlna,
        name: "Stalled TV".into(),
        host: "127.0.0.1".into(),
        port: dlna_addr.port(),
        control_url: Some(format!("http://{dlna_addr}/AVTransport/control")),
    };
    let session = ActiveCastSession::start(
        device,
        MediaPayload::Source {
            source,
            mime: "video/mp4".into(),
        },
        "Stalled Movie",
    )
    .await
    .unwrap();

    let t_stop = std::time::Instant::now();
    session.stop().await.unwrap();
    assert!(
        t_stop.elapsed() < std::time::Duration::from_secs(2),
        "stop() stalled for {:?}",
        t_stop.elapsed()
    );
    srv.abort();
}
