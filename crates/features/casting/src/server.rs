//! On-demand LAN HTTP Range server for active cast sessions (§1.3).
//! Never started unless a user initiates casting, and shuts down immediately
//! when the returned [`CastServerHandle`] is dropped or `stop()` is called.

use std::fs::File;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::Arc;

use cinma_blobstore::{ByteSource, CancellationToken};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

#[derive(Clone)]
pub enum MediaPayload {
    Source {
        source: Arc<dyn ByteSource>,
        mime: String,
    },
    LocalFile {
        path: PathBuf,
        size: u64,
        mime: String,
    },
}

impl MediaPayload {
    fn size(&self) -> u64 {
        match self {
            Self::Source { source, .. } => source.size(),
            Self::LocalFile { size, .. } => *size,
        }
    }

    fn mime(&self) -> &str {
        match self {
            Self::Source { mime, .. } | Self::LocalFile { mime, .. } => mime,
        }
    }
}

pub struct CastServerHandle {
    pub local_url: String,
    pub bind_addr: SocketAddr,
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl CastServerHandle {
    pub async fn start(payload: MediaPayload, bind_ip: Option<IpAddr>) -> io::Result<Self> {
        let host_ip = bind_ip.unwrap_or_else(detect_lan_ip);
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)).await?;
        let local_addr = listener.local_addr()?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let path_prefix = format!("/cast/{token}");
        let local_url = format!("http://{host_ip}:{}{path_prefix}", local_addr.port());
        let cancel = CancellationToken::new();
        let cancel_loop = cancel.clone();

        let task = tokio::spawn(async move {
            loop {
                let Ok(accepted) = tokio::select! (
                    () = cancel_loop.cancelled() => return,
                    res = listener.accept() => res,
                ) else {
                    break;
                };
                let (sock, _) = accepted;
                let payload = payload.clone();
                let prefix = path_prefix.clone();
                let conn_cancel = cancel_loop.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(sock, payload, &prefix, conn_cancel).await;
                });
            }
        });

        Ok(Self {
            local_url,
            bind_addr: SocketAddr::new(host_ip, local_addr.port()),
            cancel,
            task: Some(task),
        })
    }

    pub async fn stop(mut self) {
        self.cancel.cancel();
        if let Some(h) = self.task.take() {
            let _ = h.await;
        }
    }
}

impl Drop for CastServerHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(h) = self.task.take() {
            h.abort();
        }
    }
}

/// Discovers our outward LAN IPv4 address using a connected UDP socket (no packets sent).
pub fn detect_lan_ip() -> IpAddr {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0")
        && sock.connect("192.168.1.1:80").is_ok()
        && let Ok(addr) = sock.local_addr()
        && !addr.ip().is_loopback()
    {
        return addr.ip();
    }
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

async fn handle_connection(
    mut sock: TcpStream,
    payload: MediaPayload,
    expected_path: &str,
    cancel: CancellationToken,
) -> io::Result<()> {
    let mut buf = vec![0u8; 4096];
    let n = tokio::select! (
        () = cancel.cancelled() => return Ok(()),
        r = sock.read(&mut buf) => r?,
    );
    if n == 0 {
        return Ok(());
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut lines = req.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if path != expected_path || !matches!(method, "GET" | "HEAD") {
        let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = sock.write_all(resp.as_bytes()).await;
        return Ok(());
    }

    let total = payload.size();
    let range_header = lines.find_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case("range"))
            .map(|(_, v)| v.trim().to_owned())
    });

    let (start, end, is_partial) = match parse_range(range_header.as_deref(), total) {
        Some((s, e)) => (s, e, true),
        None if range_header.is_some() => {
            let resp = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nConnection: close\r\n\r\n"
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            return Ok(());
        }
        None => (0, total.saturating_sub(1), false),
    };

    let len = if total == 0 { 0 } else { end - start + 1 };
    let header = if is_partial {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: {}\r\nContent-Length: {len}\r\nContent-Range: bytes {start}-{end}/{total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
            payload.mime()
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
            payload.mime()
        )
    };
    sock.write_all(header.as_bytes()).await?;
    if method == "HEAD" || len == 0 {
        return Ok(());
    }

    stream_payload(&mut sock, payload, start, len, cancel).await
}

async fn stream_payload(
    sock: &mut TcpStream,
    payload: MediaPayload,
    mut offset: u64,
    mut remaining: u64,
    cancel: CancellationToken,
) -> io::Result<()> {
    match payload {
        MediaPayload::LocalFile { path, .. } => {
            let file = Arc::new(File::open(path)?);
            while remaining > 0 && !cancel.is_cancelled() {
                let want = remaining.min(128 * 1024) as usize;
                let f = Arc::clone(&file);
                let chunk = tokio::task::spawn_blocking(move || {
                    let mut buf = vec![0u8; want];
                    f.read_exact_at(&mut buf, offset).map(|()| buf)
                })
                .await
                .map_err(io::Error::other)??;
                sock.write_all(&chunk).await?;
                offset += want as u64;
                remaining -= want as u64;
            }
        }
        MediaPayload::Source { source, .. } => {
            let mut reader = tokio::task::spawn_blocking(move || source.open_reader())
                .await
                .map_err(io::Error::other)??;
            while remaining > 0 && !cancel.is_cancelled() {
                let want = remaining.min(128 * 1024) as usize;
                let tok = cancel.clone();
                let (r_back, res) = tokio::task::spawn_blocking(move || {
                    let mut buf = vec![0u8; want];
                    let res = reader.read_at(offset, &mut buf, &tok).map(|n| {
                        buf.truncate(n);
                        buf
                    });
                    (reader, res)
                })
                .await
                .map_err(io::Error::other)?;
                reader = r_back;
                let chunk = res?;
                if chunk.is_empty() {
                    break;
                }
                sock.write_all(&chunk).await?;
                offset += chunk.len() as u64;
                remaining -= chunk.len() as u64;
            }
        }
    }
    Ok(())
}

fn parse_range(hdr: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let spec = hdr?.strip_prefix("bytes=")?;
    if total == 0 {
        return None;
    }
    let (s_raw, e_raw) = spec.split_once('-')?;
    if s_raw.is_empty() {
        let suffix: u64 = e_raw.trim().parse().ok()?;
        let len = suffix.min(total);
        return Some((total - len, total - 1));
    }
    let start: u64 = s_raw.trim().parse().ok()?;
    let end: u64 = if e_raw.trim().is_empty() {
        total - 1
    } else {
        e_raw.trim().parse::<u64>().ok()?.min(total - 1)
    };
    (start <= end && start < total).then_some((start, end))
}
