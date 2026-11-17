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
    // Use macOS `dns-sd -B _googlecast._tcp` bounded by `timeout` (short-lived child).
    let mut child = tokio::process::Command::new("/usr/bin/dns-sd")
        .args(["-B", "_googlecast._tcp", "local"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let mut instances = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 8192];
        let deadline = tokio::time::Instant::now() + timeout.min(Duration::from_millis(1500));
        while tokio::time::Instant::now() < deadline {
            let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(rem, stdout.read(&mut buf)).await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(n)) => {
                    let s = String::from_utf8_lossy(&buf[..n]);
                    for line in s.lines() {
                        if line.contains("_googlecast._tcp.")
                            && line.contains("Add")
                            && let Some(instance) = line.split("_googlecast._tcp.").nth(1)
                        {
                            let name = instance.trim().to_owned();
                            if !name.is_empty() && !instances.contains(&name) {
                                instances.push(name);
                            }
                        }
                    }
                }
                Ok(Err(_)) => break,
            }
        }
    }
    let _ = child.kill().await;

    let mut out = Vec::new();
    for instance in instances {
        if let Some((host, port)) = resolve_chromecast_instance(&instance).await {
            out.push(CastDevice {
                id: format!("cast:{host}:{port}"),
                kind: CastKind::Chromecast,
                name: instance,
                host,
                port,
                control_url: None,
            });
        }
    }
    Ok(out)
}

async fn resolve_chromecast_instance(instance: &str) -> Option<(String, u16)> {
    let mut child = tokio::process::Command::new("/usr/bin/dns-sd")
        .args(["-L", instance, "_googlecast._tcp", "local"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut host_port = None;
    if let Some(mut stdout) = child.stdout.take() {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 4096];
        if let Ok(Ok(n)) =
            tokio::time::timeout(Duration::from_millis(800), stdout.read(&mut buf)).await
        {
            let s = String::from_utf8_lossy(&buf[..n]);
            for line in s.lines() {
                if let Some(pos) = line.find("can be reached at ") {
                    let rest = &line[pos + "can be reached at ".len()..];
                    if let Some(hp) = rest.split_whitespace().next()
                        && let Some((h, p)) = hp.rsplit_once(':')
                        && let Ok(port) = p.parse::<u16>()
                    {
                        host_port = Some((h.trim_end_matches('.').to_owned(), port));
                        break;
                    }
                }
            }
        }
    }
    let _ = child.kill().await;
    host_port
}
