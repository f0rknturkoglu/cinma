//! Minimal one-shot mDNS browse for `_googlecast._tcp.local`, in-process.
//!
//! Queries go from an ephemeral port to 224.0.0.251:5353, so responders answer
//! by unicast to us ("legacy unicast", RFC 6762 §6.7): no port 5353 binding, no
//! background listener, and the socket is dropped when discovery ends.
//! Responses are untrusted LAN input: every read is bounds-checked and
//! compression pointers are hop-limited.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;

pub const GOOGLECAST_SERVICE: &str = "_googlecast._tcp.local";
const MDNS_ADDR: &str = "224.0.0.251:5353";
const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_TXT: u16 = 16;
const TYPE_SRV: u16 = 33;
const CLASS_IN: u16 = 1;
const MAX_NAME_HOPS: usize = 16;
const MAX_NAME_LEN: usize = 255;
const MAX_RECORDS: usize = 256;

/// A resolved service instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub instance: String,
    pub friendly_name: Option<String>,
    pub ip: Ipv4Addr,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    Ptr {
        name: String,
        target: String,
    },
    Srv {
        name: String,
        port: u16,
        target: String,
    },
    A {
        name: String,
        ip: Ipv4Addr,
    },
    Txt {
        name: String,
        entries: Vec<String>,
    },
}

/// Builds a query packet with one question per `(name, type)`.
pub fn build_query(questions: &[(&str, u16)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&[0, 0, 0, 0]); // id, flags
    out.extend_from_slice(&(questions.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // an, ns, ar
    for (name, qtype) in questions {
        for label in name.trim_end_matches('.').split('.') {
            let bytes = label.as_bytes();
            let len = bytes.len().min(63);
            out.push(len as u8);
            out.extend_from_slice(&bytes[..len]);
        }
        out.push(0);
        out.extend_from_slice(&qtype.to_be_bytes());
        out.extend_from_slice(&CLASS_IN.to_be_bytes());
    }
    out
}

fn read_u16(p: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*p.get(at)?, *p.get(at + 1)?]))
}

/// Reads a (possibly compressed) name at `at`; returns it and the offset
/// just past it in the original position.
fn read_name(p: &[u8], at: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = at;
    let mut end = None;
    let mut hops = 0;
    let mut total = 0;
    loop {
        let len = *p.get(pos)? as usize;
        if len == 0 {
            end.get_or_insert(pos + 1);
            break;
        }
        if len & 0xC0 == 0xC0 {
            let ptr = (read_u16(p, pos)? & 0x3FFF) as usize;
            end.get_or_insert(pos + 2);
            hops += 1;
            if hops > MAX_NAME_HOPS || ptr >= p.len() {
                return None;
            }
            pos = ptr;
            continue;
        }
        if len & 0xC0 != 0 {
            return None;
        }
        let label = p.get(pos + 1..pos + 1 + len)?;
        total += len + 1;
        if total > MAX_NAME_LEN {
            return None;
        }
        labels.push(String::from_utf8_lossy(label).into_owned());
        pos += 1 + len;
    }
    Some((labels.join("."), end?))
}

/// Parses every answer/authority/additional record in a response packet.
pub fn parse_response(p: &[u8]) -> Option<Vec<Record>> {
    let flags = read_u16(p, 2)?;
    if flags & 0x8000 == 0 {
        return None; // a query, not a response
    }
    let qd = read_u16(p, 4)? as usize;
    let rr = read_u16(p, 6)? as usize + read_u16(p, 8)? as usize + read_u16(p, 10)? as usize;
    if qd > MAX_RECORDS || rr > MAX_RECORDS {
        return None;
    }
    let mut pos = 12;
    for _ in 0..qd {
        let (_, next) = read_name(p, pos)?;
        pos = next + 4;
    }
    let mut out = Vec::new();
    for _ in 0..rr {
        let (name, next) = read_name(p, pos)?;
        let rtype = read_u16(p, next)?;
        let rdlen = read_u16(p, next + 8)? as usize;
        let rdata = next + 10;
        let rend = rdata.checked_add(rdlen)?;
        if rend > p.len() {
            return None;
        }
        match rtype {
            TYPE_PTR => {
                if let Some((target, _)) = read_name(p, rdata) {
                    out.push(Record::Ptr { name, target });
                }
            }
            TYPE_SRV if rdlen >= 7 => {
                let port = read_u16(p, rdata + 4)?;
                if let Some((target, _)) = read_name(p, rdata + 6) {
                    out.push(Record::Srv { name, port, target });
                }
            }
            TYPE_A if rdlen == 4 => {
                let ip = Ipv4Addr::new(p[rdata], p[rdata + 1], p[rdata + 2], p[rdata + 3]);
                out.push(Record::A { name, ip });
            }
            TYPE_TXT => {
                let mut entries = Vec::new();
                let mut i = rdata;
                while i < rend {
                    let len = p[i] as usize;
                    let Some(bytes) = p.get(i + 1..(i + 1 + len).min(rend)) else {
                        break;
                    };
                    entries.push(String::from_utf8_lossy(bytes).into_owned());
                    i += 1 + len;
                }
                out.push(Record::Txt { name, entries });
            }
            _ => {}
        }
        pos = rend;
    }
    Some(out)
}

#[derive(Default)]
struct Collected {
    instances: Vec<String>,
    srv: HashMap<String, (String, u16)>,
    a: HashMap<String, Ipv4Addr>,
    txt: HashMap<String, Vec<String>>,
}

impl Collected {
    fn absorb(&mut self, service: &str, records: Vec<Record>) {
        for r in records {
            match r {
                Record::Ptr { name, target } => {
                    if name.eq_ignore_ascii_case(service) && !self.instances.contains(&target) {
                        self.instances.push(target);
                    }
                }
                Record::Srv { name, port, target } => {
                    self.srv.insert(name.to_ascii_lowercase(), (target, port));
                }
                Record::A { name, ip } => {
                    self.a.insert(name.to_ascii_lowercase(), ip);
                }
                Record::Txt { name, entries } => {
                    self.txt.insert(name.to_ascii_lowercase(), entries);
                }
            }
        }
    }

    fn resolved(&self) -> Vec<Service> {
        self.instances
            .iter()
            .filter_map(|instance| {
                let (target, port) = self.srv.get(&instance.to_ascii_lowercase())?;
                let ip = *self.a.get(&target.to_ascii_lowercase())?;
                let friendly_name = self
                    .txt
                    .get(&instance.to_ascii_lowercase())
                    .and_then(|e| e.iter().find_map(|kv| kv.strip_prefix("fn=")))
                    .map(str::to_owned);
                Some(Service {
                    instance: instance.clone(),
                    friendly_name,
                    ip,
                    port: *port,
                })
            })
            .collect()
    }

    /// Follow-up questions for instances that still lack SRV or A records.
    fn missing_questions(&self) -> Vec<(String, u16)> {
        let mut q = Vec::new();
        for instance in &self.instances {
            match self.srv.get(&instance.to_ascii_lowercase()) {
                None => {
                    q.push((instance.clone(), TYPE_SRV));
                    q.push((instance.clone(), TYPE_TXT));
                }
                Some((target, _)) if !self.a.contains_key(&target.to_ascii_lowercase()) => {
                    q.push((target.clone(), TYPE_A));
                }
                Some(_) => {}
            }
        }
        q
    }
}

async fn collect(sock: &UdpSocket, service: &str, window: Duration, into: &mut Collected) {
    let deadline = tokio::time::Instant::now() + window;
    let mut buf = vec![0u8; 9000];
    loop {
        let wait = deadline.saturating_duration_since(tokio::time::Instant::now());
        if wait.is_zero() {
            break;
        }
        match tokio::time::timeout(wait, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, _))) => {
                if let Some(records) = parse_response(&buf[..n]) {
                    into.absorb(service, records);
                }
            }
            Ok(Err(_)) | Err(_) => break,
        }
    }
}

/// Browses `service` for up to `timeout` and returns resolved instances.
pub async fn browse(service: &str, timeout: Duration) -> std::io::Result<Vec<Service>> {
    let target: SocketAddr = MDNS_ADDR
        .parse()
        .map_err(|_| std::io::Error::other("mdns address"))?;
    browse_at(service, timeout, target).await
}

/// Same as [`browse`] but against an explicit responder address (tests).
pub async fn browse_at(
    service: &str,
    timeout: Duration,
    target: SocketAddr,
) -> std::io::Result<Vec<Service>> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.send_to(&build_query(&[(service, TYPE_PTR)]), target)
        .await?;
    let mut collected = Collected::default();
    let first = timeout.mul_f32(0.7);
    collect(&sock, service, first, &mut collected).await;

    let missing = collected.missing_questions();
    if !missing.is_empty() {
        let questions: Vec<(&str, u16)> = missing.iter().map(|(n, t)| (n.as_str(), *t)).collect();
        sock.send_to(&build_query(&questions), target).await?;
        collect(
            &sock,
            service,
            timeout.saturating_sub(first),
            &mut collected,
        )
        .await;
    }
    Ok(collected.resolved())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn name(out: &mut Vec<u8>, n: &str) {
        for l in n.split('.') {
            out.push(l.len() as u8);
            out.extend_from_slice(l.as_bytes());
        }
        out.push(0);
    }

    fn rr(out: &mut Vec<u8>, owner: &[u8], rtype: u16, rdata: &[u8]) {
        out.extend_from_slice(owner);
        out.extend_from_slice(&rtype.to_be_bytes());
        out.extend_from_slice(&[0x80, 0x01, 0, 0, 0, 120]);
        out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        out.extend_from_slice(rdata);
    }

    /// A response like a Chromecast sends: PTR + SRV + TXT + A, with compression.
    pub(crate) fn sample_response() -> Vec<u8> {
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 3];
        let service_at = p.len() as u16;
        let mut service = Vec::new();
        name(&mut service, GOOGLECAST_SERVICE);
        // PTR owner = service name (written inline once, pointed to later).
        let mut instance = vec![14];
        instance.extend_from_slice(b"Chromecast-abc");
        instance.extend_from_slice(&(0xC000 | service_at).to_be_bytes());
        rr(&mut p, &service, TYPE_PTR, &instance);
        let instance_at = (p.len() - instance.len()) as u16;
        let inst_ptr = (0xC000 | instance_at).to_be_bytes();
        let mut srv = vec![0, 0, 0, 0, 0x1F, 0x49];
        name(&mut srv, "cc-abc.local");
        rr(&mut p, &inst_ptr, TYPE_SRV, &srv);
        let mut txt = Vec::new();
        for e in ["id=abc", "fn=Living Room TV"] {
            txt.push(e.len() as u8);
            txt.extend_from_slice(e.as_bytes());
        }
        rr(&mut p, &inst_ptr, TYPE_TXT, &txt);
        let mut host = Vec::new();
        name(&mut host, "cc-abc.local");
        rr(&mut p, &host, TYPE_A, &[192, 168, 1, 40]);
        p
    }

    #[test]
    fn parses_a_compressed_chromecast_response() {
        let records = parse_response(&sample_response()).unwrap();
        let mut c = Collected::default();
        c.absorb(GOOGLECAST_SERVICE, records);
        assert_eq!(
            c.resolved(),
            vec![Service {
                instance: format!("Chromecast-abc.{GOOGLECAST_SERVICE}"),
                friendly_name: Some("Living Room TV".into()),
                ip: Ipv4Addr::new(192, 168, 1, 40),
                port: 8009,
            }]
        );
        assert!(c.missing_questions().is_empty());
    }

    #[test]
    fn rejects_pointer_loops_and_truncation() {
        // Answer whose owner name points at itself.
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xC0, 12];
        p.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 1, 0, 4, 1, 2, 3, 4]);
        assert!(parse_response(&p).is_none());
        let full = sample_response();
        for cut in [0, 5, 12, 20, full.len() - 3] {
            let _ = parse_response(&full[..cut]); // must not panic
        }
        // Queries are ignored.
        assert!(parse_response(&build_query(&[(GOOGLECAST_SERVICE, TYPE_PTR)])).is_none());
    }

    #[tokio::test]
    async fn browses_a_local_responder_over_udp() {
        let responder = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = responder.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (_, from) = responder.recv_from(&mut buf).await.unwrap();
            responder.send_to(&sample_response(), from).await.unwrap();
        });
        let found = browse_at(GOOGLECAST_SERVICE, Duration::from_millis(400), addr)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].port, 8009);
    }
}
