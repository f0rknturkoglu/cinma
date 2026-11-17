//! On-demand LAN casting (Chromecast & DLNA) + ephemeral HTTP Range streamer (§1.3).
//! Zero sockets or servers run until casting is explicitly started.

#![forbid(unsafe_code)]

pub mod chromecast;
pub mod discovery;
pub mod dlna;
pub mod server;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use chromecast::CastV2Client;
use dlna::DlnaClient;
use server::{CastServerHandle, MediaPayload};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CastKind {
    Chromecast,
    Dlna,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CastDevice {
    pub id: String,
    pub kind: CastKind,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub control_url: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CastError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Invalid(String),
}

pub async fn discover_lan_devices(timeout: Duration) -> Result<Vec<CastDevice>, CastError> {
    discovery::discover_devices(timeout).await
}

enum RemoteTransport {
    Dlna(DlnaClient),
    Chromecast(Box<CastV2Client>),
}

pub struct ActiveCastSession {
    pub device: CastDevice,
    server: Mutex<Option<CastServerHandle>>,
    transport: Mutex<RemoteTransport>,
}

impl ActiveCastSession {
    pub async fn start(
        device: CastDevice,
        payload: MediaPayload,
        title: &str,
    ) -> Result<Self, CastError> {
        let mime = match &payload {
            MediaPayload::Source { mime, .. } | MediaPayload::LocalFile { mime, .. } => {
                mime.clone()
            }
        };
        let server = CastServerHandle::start(payload, None).await?;
        let media_url = server.local_url.clone();

        let transport = match device.kind {
            CastKind::Dlna => {
                let control = device
                    .control_url
                    .clone()
                    .ok_or_else(|| CastError::Invalid("missing DLNA control URL".into()))?;
                let dlna = DlnaClient::new(control)?;
                dlna.set_av_transport_uri(&media_url, title).await?;
                dlna.play().await?;
                RemoteTransport::Dlna(dlna)
            }
            CastKind::Chromecast => {
                let mut cc = CastV2Client::connect(&device.host, device.port).await?;
                cc.launch_and_load(&media_url, title, &mime).await?;
                RemoteTransport::Chromecast(Box::new(cc))
            }
        };

        Ok(Self {
            device,
            server: Mutex::new(Some(server)),
            transport: Mutex::new(transport),
        })
    }

    pub async fn play(&self) -> Result<(), CastError> {
        let mut t = self.transport.lock().await;
        match &mut *t {
            RemoteTransport::Dlna(d) => d.play().await,
            RemoteTransport::Chromecast(c) => c.media_command("PLAY", None).await,
        }
    }

    pub async fn pause(&self) -> Result<(), CastError> {
        let mut t = self.transport.lock().await;
        match &mut *t {
            RemoteTransport::Dlna(d) => d.pause().await,
            RemoteTransport::Chromecast(c) => c.media_command("PAUSE", None).await,
        }
    }

    pub async fn seek(&self, seconds: f64) -> Result<(), CastError> {
        let mut t = self.transport.lock().await;
        match &mut *t {
            RemoteTransport::Dlna(d) => d.seek_seconds(seconds.max(0.0) as u64).await,
            RemoteTransport::Chromecast(c) => {
                c.media_command(
                    "SEEK",
                    Some(serde_json::json!({ "currentTime": seconds.max(0.0) })),
                )
                .await
            }
        }
    }

    pub async fn stop(&self) -> Result<(), CastError> {
        {
            let mut t = self.transport.lock().await;
            match &mut *t {
                RemoteTransport::Dlna(d) => {
                    let _ = d.stop().await;
                }
                RemoteTransport::Chromecast(c) => {
                    let _ = c.media_command("STOP", None).await;
                }
            }
        }
        if let Some(srv) = self.server.lock().await.take() {
            srv.stop().await;
        }
        Ok(())
    }
}
