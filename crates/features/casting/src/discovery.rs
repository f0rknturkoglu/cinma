//! Chromecast & DLNA discovery (executed only when explicitly requested).

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use url::Url;

use crate::{CastDevice, CastError, CastKind};

const SSDP_ADDR: &str = "239.255.255.250:1900";
const SSDP_MSEARCH: &str = "M-SEARCH * HTTP/1.1\r\n\
HOST: 239.255.255.250:1900\r\n\
MAN: \"ssdp:discover\"\r\n\
MX: 1\r\n\
ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";

pub async fn discover_devices(timeout: Duration) -> Result<Vec<CastDevice>, CastError> {
    let (dlna_res, cast_res) = tokio::join!(discover_dlna(timeout), discover_chromecast(timeout));
    let mut devices = Vec::new();
    if let Ok(mut d) = dlna_res {
        devices.append(&mut d);
    }
    if let Ok(mut c) = cast_res {
        devices.append(&mut c);
    }
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices.dedup_by(|a, b| a.id == b.id);
    Ok(devices)
}

async fn discover_dlna(timeout: Duration) -> Result<Vec<CastDevice>, CastError> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    let target: SocketAddr = SSDP_ADDR
        .parse()
        .map_err(|_| CastError::Invalid("ssdp".into()))?;
    sock.send_to(SSDP_MSEARCH.as_bytes(), target).await?;

    let mut locations = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut buf = [0u8; 4096];
    while tokio::time::Instant::now() < deadline {
        let wait = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Ok((n, _))) = tokio::time::timeout(wait, sock.recv_from(&mut buf)).await else {
            break;
        };
        let msg = String::from_utf8_lossy(&buf[..n]);
        for line in msg.lines() {
            if let Some((k, v)) = line.split_once(':')
                && k.trim().eq_ignore_ascii_case("location")
            {
                let loc = v.trim().to_owned();
                if !locations.contains(&loc) {
                    locations.push(loc);
                }
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    let mut out = Vec::new();
    for loc in locations {
        if let Ok(dev) = fetch_dlna_descriptor(&client, &loc).await {
            out.push(dev);
        }
    }
    Ok(out)
}

async fn fetch_dlna_descriptor(
    client: &reqwest::Client,
    location: &str,
) -> Result<CastDevice, CastError> {
    let xml = client.get(location).send().await?.text().await?;
    let base = Url::parse(location).map_err(|e| CastError::Invalid(e.to_string()))?;
    let name = extract_xml_tag(&xml, "friendlyName")
        .unwrap_or_else(|| base.host_str().unwrap_or("DLNA Renderer").to_owned());
    let control = extract_avtransport_control_url(&xml)
        .and_then(|rel| base.join(&rel).ok())
        .ok_or_else(|| CastError::Invalid("missing AVTransport controlURL".into()))?;
    Ok(CastDevice {
        id: format!("dlna:{location}"),
        kind: CastKind::Dlna,
        name,
        host: base.host_str().unwrap_or_default().to_owned(),
        port: base.port_or_known_default().unwrap_or(80),
        control_url: Some(control.to_string()),
    })
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_owned())
}

fn extract_avtransport_control_url(xml: &str) -> Option<String> {
    let marker = "urn:schemas-upnp-org:service:AVTransport:1";
    let pos = xml.find(marker)?;
    extract_xml_tag(&xml[pos..], "controlURL")
}

async fn discover_chromecast(timeout: Duration) -> Result<Vec<CastDevice>, CastError> {
    // In-process mDNS (no `dns-sd` child process), bounded by `timeout`.
    let services = crate::mdns::browse(
        crate::mdns::GOOGLECAST_SERVICE,
        timeout.min(Duration::from_millis(2000)),
    )
    .await?;
    Ok(services
        .into_iter()
        .map(|s| {
            let suffix = format!(".{}", crate::mdns::GOOGLECAST_SERVICE);
            let name = s.friendly_name.clone().unwrap_or_else(|| {
                s.instance
                    .strip_suffix(&suffix)
                    .unwrap_or(&s.instance)
                    .to_owned()
            });
            CastDevice {
                id: format!("cast:{}:{}", s.ip, s.port),
                kind: CastKind::Chromecast,
                name,
                host: s.ip.to_string(),
                port: s.port,
                control_url: None,
            }
        })
        .collect())
}
