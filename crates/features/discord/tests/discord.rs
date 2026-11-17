#![allow(clippy::unwrap_used, clippy::expect_used)]

use cinma_discord::{DiscordPresenceClient, PresenceActivity};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

async fn read_ipc_frame(sock: &mut tokio::net::UnixStream) -> (u32, serde_json::Value) {
    let mut hdr = [0u8; 8];
    sock.read_exact(&mut hdr).await.unwrap();
    let op = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    let mut body = vec![0u8; len];
    sock.read_exact(&mut body).await.unwrap();
    (op, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn discord_ipc_handshake_activity_and_clear() {
    let dir = tempfile::tempdir().unwrap();
    let sock_path = dir.path().join("discord-ipc-0");
    let listener = UnixListener::bind(&sock_path).unwrap();

    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let (op0, hs) = read_ipc_frame(&mut s).await;
        assert_eq!(op0, 0);
        assert_eq!(hs["v"], 1);
        let ready = br#"{"cmd":"DISPATCH","evt":"READY"}"#;
        let mut resp_hdr = [0u8; 8];
        resp_hdr[..4].copy_from_slice(&1u32.to_le_bytes());
        resp_hdr[4..8].copy_from_slice(&(ready.len() as u32).to_le_bytes());
        s.write_all(&resp_hdr).await.unwrap();
        s.write_all(ready).await.unwrap();

        let (op1, f1) = read_ipc_frame(&mut s).await;
        assert_eq!(op1, 1);
        assert_eq!(f1["cmd"], "SET_ACTIVITY");
        assert_eq!(f1["args"]["activity"]["details"], "Severance S01E01");

        let (op2, f2) = read_ipc_frame(&mut s).await;
        assert_eq!(op2, 1);
        assert!(f2["args"]["activity"].is_null());

        let (op3, _) = read_ipc_frame(&mut s).await;
        assert_eq!(op3, 2);
    });

    let mut client = DiscordPresenceClient::with_socket_path(sock_path, None);
    client
        .set_activity(&PresenceActivity {
            details: "Severance S01E01".into(),
            state: Some("Good News About Hell".into()),
            start_ts: Some(1000),
            end_ts: Some(4600),
            paused: false,
        })
        .await
        .unwrap();
    client.clear().await.unwrap();
    server.await.unwrap();
}
