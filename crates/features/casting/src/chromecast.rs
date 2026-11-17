//! Minimal Chromecast CastV2 protocol client over TLS port 8009 (no external protobuf codegen).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tokio_rustls::{TlsConnector, client::TlsStream};

use crate::CastError;

const RECEIVER_APP_ID: &str = "CC1AD845";
const NS_CONN: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

struct CastSessionState {
    transport_id: Option<String>,
    media_session_id: Option<i64>,
}

pub struct CastV2Client {
    writer: Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>,
    state: Arc<Mutex<CastSessionState>>,
    notify: Arc<Notify>,
    closed: Arc<AtomicBool>,
    reader_task: JoinHandle<()>,
    req_id: AtomicI64,
}

impl Drop for CastV2Client {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        self.reader_task.abort();
    }
}

impl CastV2Client {
    pub async fn connect(host: &str, port: u16) -> Result<Self, CastError> {
        let mut tls_cfg =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| CastError::Invalid(e.to_string()))?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(CastLanCertVerifier))
                .with_no_client_auth();
        tls_cfg.enable_sni = false;

        let connector = TlsConnector::from(Arc::new(tls_cfg));
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
            .await
            .map_err(|_| CastError::Invalid("cast TCP connect timed out".into()))??;
        let _ = tcp.set_nodelay(true);
        let domain = ServerName::try_from("chromecast.local".to_owned())
            .map_err(|e| CastError::Invalid(e.to_string()))?;
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, connector.connect(domain, tcp))
            .await
            .map_err(|_| CastError::Invalid("cast TLS handshake timed out".into()))?
            .map_err(|e| CastError::Invalid(format!("cast TLS: {e}")))?;

        let (reader, writer) = tokio::io::split(stream);
        let writer = Arc::new(Mutex::new(writer));
        let state = Arc::new(Mutex::new(CastSessionState {
            transport_id: None,
            media_session_id: None,
        }));
        let notify = Arc::new(Notify::new());
        let closed = Arc::new(AtomicBool::new(false));

        let reader_task = tokio::spawn(pump_incoming(
            reader,
            Arc::clone(&writer),
            Arc::clone(&state),
            Arc::clone(&notify),
            Arc::clone(&closed),
        ));

        let client = Self {
            writer,
            state,
            notify,
            closed,
            reader_task,
            req_id: AtomicI64::new(1),
        };
        client
            .send_json("receiver-0", NS_CONN, r#"{"type":"CONNECT"}"#)
            .await?;
        Ok(client)
    }

    pub async fn launch_and_load(
        &mut self,
        url: &str,
        title: &str,
        mime: &str,
    ) -> Result<(), CastError> {
        let id = self.req_id.fetch_add(1, Ordering::Relaxed);
        let launch = serde_json::json!({
            "type": "LAUNCH",
            "requestId": id,
            "appId": RECEIVER_APP_ID,
        });
        self.send_json("receiver-0", NS_RECEIVER, &launch.to_string())
            .await?;

        let tid = self.wait_for_transport_id(Duration::from_secs(3)).await;
        if let Some(ref tid) = tid {
            self.send_json(tid, NS_CONN, r#"{"type":"CONNECT"}"#)
                .await?;
        }

        let dest = tid.unwrap_or_else(|| "receiver-0".to_owned());
        let load_id = self.req_id.fetch_add(1, Ordering::Relaxed);
        let load = serde_json::json!({
            "type": "LOAD",
            "requestId": load_id,
            "autoplay": true,
            "media": {
                "contentId": url,
                "streamType": "BUFFERED",
                "contentType": mime,
                "metadata": { "metadataType": 0, "title": title }
            }
        });
        self.send_json(&dest, NS_MEDIA, &load.to_string()).await?;
        // Capture real mediaSessionId from the receiver's MEDIA_STATUS if sent promptly.
        let _ = self
            .wait_for_media_session_id(Duration::from_millis(800))
            .await;
        Ok(())
    }

    async fn wait_for_transport_id(&self, timeout: Duration) -> Option<String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(tid) = self.state.lock().await.transport_id.clone() {
                return Some(tid);
            }
            if self.closed.load(Ordering::Relaxed) {
                return None;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let _ = tokio::time::timeout(remaining, self.notify.notified()).await;
        }
    }

    async fn wait_for_media_session_id(&self, timeout: Duration) -> Option<i64> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(msid) = self.state.lock().await.media_session_id {
                return Some(msid);
            }
            if self.closed.load(Ordering::Relaxed) {
                return None;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let _ = tokio::time::timeout(remaining, self.notify.notified()).await;
        }
    }

    pub async fn media_command(
        &mut self,
        kind: &str,
        extra: Option<serde_json::Value>,
    ) -> Result<(), CastError> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(CastError::Invalid(
                "cast connection closed by receiver".into(),
            ));
        }
        let (dest, msid) = {
            let st = self.state.lock().await;
            (
                st.transport_id
                    .clone()
                    .unwrap_or_else(|| "receiver-0".to_owned()),
                st.media_session_id.unwrap_or(1),
            )
        };
        let id = self.req_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = serde_json::json!({
            "type": kind,
            "requestId": id,
            "mediaSessionId": msid,
        });
        if let (Some(obj), Some(serde_json::Value::Object(map))) = (msg.as_object_mut(), extra) {
            for (k, v) in map {
                obj.insert(k, v);
            }
        }
        self.send_json(&dest, NS_MEDIA, &msg.to_string()).await
    }

    async fn send_json(&self, dest: &str, namespace: &str, json: &str) -> Result<(), CastError> {
        send_frame(&self.writer, dest, namespace, json).await
    }
}

async fn send_frame(
    writer: &Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>,
    dest: &str,
    namespace: &str,
    json: &str,
) -> Result<(), CastError> {
    let frame = encode_cast_message("sender-0", dest, namespace, json);
    let len = (frame.len() as u32).to_be_bytes();
    tokio::time::timeout(WRITE_TIMEOUT, async {
        let mut w = writer.lock().await;
        w.write_all(&len).await?;
        w.write_all(&frame).await?;
        w.flush().await?;
        Ok::<_, std::io::Error>(())
    })
    .await
    .map_err(|_| CastError::Invalid("cast write timed out".into()))?
    .map_err(CastError::Io)
}

async fn recv_frame<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<DecodedCastFrame, CastError> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 1024 * 1024 {
        return Err(CastError::Invalid("oversized cast frame".into()));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(decode_cast_message(&buf))
}

async fn pump_incoming(
    mut reader: ReadHalf<TlsStream<TcpStream>>,
    writer: Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>,
    state: Arc<Mutex<CastSessionState>>,
    notify: Arc<Notify>,
    closed: Arc<AtomicBool>,
) {
    while let Ok(msg) = recv_frame(&mut reader).await {
        let Some(val) = msg.payload_json else {
            continue;
        };
        if msg.namespace == NS_HEARTBEAT && val.get("type").and_then(|v| v.as_str()) == Some("PING")
        {
            let _ = send_frame(&writer, "receiver-0", NS_HEARTBEAT, r#"{"type":"PONG"}"#).await;
            continue;
        }
        let mut changed = false;
        {
            let mut st = state.lock().await;
            if let Some(tid) = val
                .pointer("/status/applications/0/transportId")
                .and_then(|v| v.as_str())
                && st.transport_id.as_deref() != Some(tid)
            {
                st.transport_id = Some(tid.to_owned());
                changed = true;
            }
            if let Some(msid) = val
                .pointer("/status/0/mediaSessionId")
                .and_then(|v| v.as_i64())
                && st.media_session_id != Some(msid)
            {
                st.media_session_id = Some(msid);
                changed = true;
            }
        }
        if changed {
            notify.notify_waiters();
        }
    }
    closed.store(true, Ordering::Relaxed);
    notify.notify_waiters();
}

pub struct DecodedCastFrame {
    pub namespace: String,
    pub payload_json: Option<serde_json::Value>,
}

/// Encodes a protobuf `extensions.api.cast_channel.CastMessage`:
/// 1: protocol_version=0, 2: source_id, 3: destination_id, 4: namespace,
/// 5: payload_type=0 (STRING), 6: payload_utf8.
pub fn encode_cast_message(src: &str, dst: &str, namespace: &str, payload: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + payload.len());
    // field 1 (varint 0)
    out.extend_from_slice(&[0x08, 0x00]);
    push_str_field(&mut out, 2, src);
    push_str_field(&mut out, 3, dst);
    push_str_field(&mut out, 4, namespace);
    // field 5 (varint 0 = STRING)
    out.extend_from_slice(&[0x28, 0x00]);
    push_str_field(&mut out, 6, payload);
    out
}

pub fn decode_cast_message(bytes: &[u8]) -> DecodedCastFrame {
    let mut pos = 0;
    let mut namespace = String::new();
    let mut payload_json = None;
    while pos < bytes.len() {
        let tag = bytes[pos];
        pos += 1;
        let field = tag >> 3;
        let wire = tag & 0x07;
        match wire {
            0 => {
                while pos < bytes.len() && (bytes[pos] & 0x80) != 0 {
                    pos += 1;
                }
                pos += 1;
            }
            2 => {
                let Some((len, used)) = read_varint(&bytes[pos..]) else {
                    break;
                };
                pos += used;
                if pos + len > bytes.len() {
                    break;
                }
                let slice = &bytes[pos..pos + len];
                pos += len;
                if field == 4 {
                    namespace = String::from_utf8_lossy(slice).into_owned();
                } else if field == 6 {
                    payload_json = serde_json::from_slice(slice).ok();
                }
            }
            _ => break,
        }
    }
    DecodedCastFrame {
        namespace,
        payload_json,
    }
}

fn push_str_field(out: &mut Vec<u8>, field: u8, s: &str) {
    out.push((field << 3) | 2);
    let mut len = s.len();
    loop {
        let mut b = (len & 0x7F) as u8;
        len >>= 7;
        if len != 0 {
            b |= 0x80;
        }
        out.push(b);
        if len == 0 {
            break;
        }
    }
    out.extend_from_slice(s.as_bytes());
}

fn read_varint(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut val = 0usize;
    for (i, &b) in bytes.iter().enumerate().take(5) {
        val |= ((b & 0x7F) as usize) << (i * 7);
        if (b & 0x80) == 0 {
            return Some((val, i + 1));
        }
    }
    None
}

/// Chromecast devices authenticate on LAN via ephemeral self-signed certs;
/// scoped exclusively to explicit user CastV2 sessions.
#[derive(Debug)]
struct CastLanCertVerifier;

impl ServerCertVerifier for CastLanCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}
