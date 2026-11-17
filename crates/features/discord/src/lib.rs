//! In-process Discord Rich Presence over Discord's local IPC Unix socket
//! (`$TMPDIR/discord-ipc-{0..9}`). Zero sidecars; constructed lazily only when
//! `Feature::Discord` is enabled (`CLAUDE.md` §1.5).

#![forbid(unsafe_code)]

use std::path::PathBuf;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Public Discord application client ID for Rich Presence IPC framing.
const DEFAULT_CLIENT_ID: &str = "1310000000000000000";

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Discord IPC not running")]
    NotRunning,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresenceActivity {
    pub details: String,
    pub state: Option<String>,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub paused: bool,
}

pub struct DiscordPresenceClient {
    socket: Option<UnixStream>,
    nonce: u64,
    override_path: Option<PathBuf>,
    client_id: String,
}

impl DiscordPresenceClient {
    pub fn new(client_id: Option<String>) -> Self {
        Self {
            socket: None,
            nonce: 0,
            override_path: None,
            client_id: client_id
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_owned()),
        }
    }

    pub fn with_socket_path(path: PathBuf, client_id: Option<String>) -> Self {
        Self {
            socket: None,
            nonce: 0,
            override_path: Some(path),
            client_id: client_id
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_owned()),
        }
    }

    async fn ensure_connected(&mut self) -> Result<&mut UnixStream, DiscordError> {
        if self.socket.is_none() {
            let stream = self.connect_any().await?;
            let handshake = serde_json::json!({
                "v": 1,
                "client_id": self.client_id,
            });
            let mut stream = stream;
            write_frame(&mut stream, OP_HANDSHAKE, &serde_json::to_vec(&handshake)?).await?;
            let _ = read_frame_header_ignore_payload(&mut stream).await;
            self.socket = Some(stream);
        }
        self.socket.as_mut().ok_or(DiscordError::NotRunning)
    }

    async fn connect_any(&self) -> Result<UnixStream, DiscordError> {
        if let Some(p) = &self.override_path {
            return Ok(UnixStream::connect(p).await?);
        }
        for dir in candidate_ipc_dirs() {
            for idx in 0..10 {
                let candidate = dir.join(format!("discord-ipc-{idx}"));
                if let Ok(stream) = UnixStream::connect(&candidate).await {
                    return Ok(stream);
                }
            }
        }
        Err(DiscordError::NotRunning)
    }

    pub async fn set_activity(&mut self, act: &PresenceActivity) -> Result<(), DiscordError> {
        self.nonce += 1;
        let nonce = format!("cinma-{}", self.nonce);
        let mut activity = serde_json::Map::new();
        activity.insert("details".into(), serde_json::json!(act.details));
        if let Some(state) = &act.state {
            activity.insert("state".into(), serde_json::json!(state));
        }
        if !act.paused && (act.start_ts.is_some() || act.end_ts.is_some()) {
            let mut ts = serde_json::Map::new();
            if let Some(s) = act.start_ts {
                ts.insert("start".into(), serde_json::json!(s));
            }
            if let Some(e) = act.end_ts {
                ts.insert("end".into(), serde_json::json!(e));
            }
            activity.insert("timestamps".into(), serde_json::Value::Object(ts));
        }
        activity.insert(
            "assets".into(),
            serde_json::json!({
                "large_text": "Cinma",
                "small_text": if act.paused { "Paused" } else { "Playing" }
            }),
        );

        let frame = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": std::process::id(),
                "activity": serde_json::Value::Object(activity),
            },
            "nonce": nonce,
        });
        let bytes = serde_json::to_vec(&frame)?;
        let res = {
            let sock = self.ensure_connected().await?;
            write_frame(sock, OP_FRAME, &bytes).await
        };
        if res.is_err() {
            self.socket = None;
        }
        Ok(res?)
    }

    pub async fn clear(&mut self) -> Result<(), DiscordError> {
        let Some(mut sock) = self.socket.take() else {
            return Ok(());
        };
        self.nonce += 1;
        let frame = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id() },
            "nonce": format!("cinma-{}", self.nonce),
        });
        let _ = write_frame(&mut sock, OP_FRAME, &serde_json::to_vec(&frame)?).await;
        let _ = write_frame(&mut sock, OP_CLOSE, b"{}").await;
        let _ = sock.shutdown().await;
        Ok(())
    }
}

fn candidate_ipc_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in ["TMPDIR", "XDG_RUNTIME_DIR", "TMP", "TEMP"] {
        if let Some(val) = std::env::var_os(var)
            && !val.is_empty()
        {
            out.push(PathBuf::from(val));
        }
    }
    out.push(PathBuf::from("/tmp"));
    out
}

async fn write_frame(sock: &mut UnixStream, op: u32, payload: &[u8]) -> std::io::Result<()> {
    let mut header = [0u8; 8];
    header[..4].copy_from_slice(&op.to_le_bytes());
    header[4..8].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    sock.write_all(&header).await?;
    sock.write_all(payload).await?;
    sock.flush().await
}

async fn read_frame_header_ignore_payload(sock: &mut UnixStream) -> std::io::Result<()> {
    let mut header = [0u8; 8];
    sock.read_exact(&mut header).await?;
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if len > 0 && len <= 64 * 1024 {
        let mut payload = vec![0u8; len];
        sock.read_exact(&mut payload).await?;
    }
    Ok(())
}
