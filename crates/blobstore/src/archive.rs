//! Store-mode archive wrapper (`ByteSource` wrapping `ByteSource`, `CLAUDE.md` §5.1).
//! Supports uncompressed (`method == 0` / `0x30`) entries inside ZIP, RAR4, and
//! RAR5 containers so video files inside multi-file or archived torrents stream
//! directly with zero full extraction and full seek priority forwarding.

use std::io;
use std::sync::Arc;

use crate::{ByteSource, CancellationToken, RangeSet, SourceReader};

const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "wmv",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Rar4,
    Rar5,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub name: String,
    pub data_offset: u64,
    pub size: u64,
    pub kind: ArchiveKind,
}

/// Detects whether a filename ends with `.zip` or `.rar`.
pub fn detect_archive_kind(filename: Option<&str>) -> Option<ArchiveKind> {
    let ext = filename?.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "zip" => Some(ArchiveKind::Zip),
        "rar" => Some(ArchiveKind::Rar4),
        _ => None,
    }
}

fn is_video_name(name: &str) -> bool {
    let Some(ext) = name.rsplit('.').next() else {
        return false;
    };
    let lower = ext.to_ascii_lowercase();
    VIDEO_EXTENSIONS.iter().any(|e| *e == lower)
}

fn read_exact_at(
    reader: &mut dyn SourceReader,
    mut offset: u64,
    mut dst: &mut [u8],
    cancel: &CancellationToken,
) -> io::Result<()> {
    while !dst.is_empty() {
        let n = reader.read_at(offset, dst, cancel)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF reading archive header",
            ));
        }
        offset += n as u64;
        dst = &mut dst[n..];
    }
    Ok(())
}

fn le_u16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le_u64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Scans an outer `ByteSource` for store-mode (uncompressed) entries in ZIP,
/// RAR4, or RAR5 containers. Compressed entries are skipped.
pub fn list_store_entries(
    outer: &dyn ByteSource,
    cancel: &CancellationToken,
) -> io::Result<Vec<ArchiveEntry>> {
    let total = outer.size();
    if total < 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "archive too small",
        ));
    }
    let mut reader = outer.open_reader()?;
    let mut magic = [0u8; 8];
    read_exact_at(&mut *reader, 0, &mut magic, cancel)?;

    if &magic[..4] == b"PK\x03\x04" || &magic[..4] == b"PK\x05\x06" {
        return list_zip_store_entries(&mut *reader, total, cancel);
    }
    if &magic[..8] == b"Rar!\x1A\x07\x01\x00" {
        return list_rar5_store_entries(&mut *reader, total, cancel);
    }
    if &magic[..7] == b"Rar!\x1A\x07\x00" {
        return list_rar4_store_entries(&mut *reader, total, cancel);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unsupported archive magic (expected store-mode ZIP or RAR)",
    ))
}

fn list_zip_store_entries(
    reader: &mut dyn SourceReader,
    total: u64,
    cancel: &CancellationToken,
) -> io::Result<Vec<ArchiveEntry>> {
    // End of Central Directory sits within the last 64 KiB + 22 bytes.
    let tail_len = total.min(65_557) as usize;
    let tail_off = total - tail_len as u64;
    let mut tail = vec![0u8; tail_len];
    read_exact_at(reader, tail_off, &mut tail, cancel)?;

    let eocd_pos = (0..=tail_len.saturating_sub(22))
        .rev()
        .find(|&i| &tail[i..i + 4] == b"PK\x05\x06")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ZIP EOCD not found"))?;

    let mut cd_size = le_u32(&tail[eocd_pos + 12..eocd_pos + 16]) as u64;
    let mut cd_offset = le_u32(&tail[eocd_pos + 16..eocd_pos + 20]) as u64;
    let mut entry_count = le_u16(&tail[eocd_pos + 10..eocd_pos + 12]) as u64;

    // Check ZIP64 EOCD locator immediately before EOCD if standard values are 0xFFFF / 0xFFFFFFFF.
    if (cd_offset == u32::MAX as u64 || entry_count == u16::MAX as u64) && eocd_pos >= 20 {
        let loc_pos = eocd_pos - 20;
        if &tail[loc_pos..loc_pos + 4] == b"PK\x06\x07" {
            let z64_off = le_u64(&tail[loc_pos + 8..loc_pos + 16]);
            if z64_off + 56 <= total {
                let mut z64 = [0u8; 56];
                read_exact_at(reader, z64_off, &mut z64, cancel)?;
                if &z64[..4] == b"PK\x06\x06" {
                    entry_count = le_u64(&z64[32..40]);
                    cd_size = le_u64(&z64[40..48]);
                    cd_offset = le_u64(&z64[48..56]);
                }
            }
        }
    }

    if cd_offset + cd_size > total || cd_size > 32 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ZIP central directory out of bounds",
        ));
    }

    let mut cd = vec![0u8; cd_size as usize];
    read_exact_at(reader, cd_offset, &mut cd, cancel)?;

    let mut out = Vec::new();
    let mut pos = 0usize;
    for _ in 0..entry_count {
        if pos + 46 > cd.len() || &cd[pos..pos + 4] != b"PK\x01\x02" {
            break;
        }
        let method = le_u16(&cd[pos + 10..pos + 12]);
        let mut comp_size = le_u32(&cd[pos + 20..pos + 24]) as u64;
        let mut uncomp_size = le_u32(&cd[pos + 24..pos + 28]) as u64;
        let name_len = le_u16(&cd[pos + 28..pos + 30]) as usize;
        let extra_len = le_u16(&cd[pos + 30..pos + 32]) as usize;
        let comment_len = le_u16(&cd[pos + 32..pos + 34]) as usize;
        let mut lfh_offset = le_u32(&cd[pos + 42..pos + 46]) as u64;

        let hdr_end = pos + 46 + name_len + extra_len + comment_len;
        if hdr_end > cd.len() {
            break;
        }
        let name = String::from_utf8_lossy(&cd[pos + 46..pos + 46 + name_len]).into_owned();
        let extra = &cd[pos + 46 + name_len..pos + 46 + name_len + extra_len];

        // Parse ZIP64 extended information extra field (tag 0x0001) when needed.
        if (uncomp_size == u32::MAX as u64
            || comp_size == u32::MAX as u64
            || lfh_offset == u32::MAX as u64)
            && let Some((z_uncomp, z_comp, z_off)) = parse_zip64_extra(
                extra,
                uncomp_size == u32::MAX as u64,
                comp_size == u32::MAX as u64,
                lfh_offset == u32::MAX as u64,
            )
        {
            if let Some(v) = z_uncomp {
                uncomp_size = v;
            }
            if let Some(v) = z_comp {
                comp_size = v;
            }
            if let Some(v) = z_off {
                lfh_offset = v;
            }
        }

        // Only uncompressed STORE entries (method 0) can be streamed without decompression.
        if method == 0 && !name.ends_with('/') && uncomp_size > 0 && lfh_offset + 30 <= total {
            let mut lfh = [0u8; 30];
            read_exact_at(reader, lfh_offset, &mut lfh, cancel)?;
            if &lfh[..4] == b"PK\x03\x04" {
                let lfh_name_len = le_u16(&lfh[26..28]) as u64;
                let lfh_extra_len = le_u16(&lfh[28..30]) as u64;
                let data_offset = lfh_offset + 30 + lfh_name_len + lfh_extra_len;
                let size = uncomp_size.max(comp_size);
                if data_offset + size <= total {
                    out.push(ArchiveEntry {
                        name,
                        data_offset,
                        size,
                        kind: ArchiveKind::Zip,
                    });
                }
            }
        }
        pos = hdr_end;
    }

    Ok(out)
}

fn parse_zip64_extra(
    extra: &[u8],
    need_uncomp: bool,
    need_comp: bool,
    need_off: bool,
) -> Option<(Option<u64>, Option<u64>, Option<u64>)> {
    let mut i = 0;
    while i + 4 <= extra.len() {
        let tag = le_u16(&extra[i..i + 2]);
        let sz = le_u16(&extra[i + 2..i + 4]) as usize;
        if i + 4 + sz > extra.len() {
            break;
        }
        if tag == 0x0001 {
            let field = &extra[i + 4..i + 4 + sz];
            let mut p = 0;
            let uncomp = if need_uncomp && p + 8 <= field.len() {
                let v = le_u64(&field[p..p + 8]);
                p += 8;
                Some(v)
            } else {
                None
            };
            let comp = if need_comp && p + 8 <= field.len() {
                let v = le_u64(&field[p..p + 8]);
                p += 8;
                Some(v)
            } else {
                None
            };
            let off = if need_off && p + 8 <= field.len() {
                Some(le_u64(&field[p..p + 8]))
            } else {
                None
            };
            return Some((uncomp, comp, off));
        }
        i += 4 + sz;
    }
    None
}

fn list_rar4_store_entries(
    reader: &mut dyn SourceReader,
    total: u64,
    cancel: &CancellationToken,
) -> io::Result<Vec<ArchiveEntry>> {
    let mut out = Vec::new();
    let mut pos = 7u64; // Past "Rar!\x1A\x07\x00"
    while pos + 7 <= total {
        let mut base = [0u8; 7];
        read_exact_at(reader, pos, &mut base, cancel)?;
        let htype = base[2];
        let flags = le_u16(&base[3..5]);
        let hsize = le_u16(&base[5..7]) as u64;
        if hsize < 7 || pos + hsize > total {
            break;
        }
        let add_size = if (flags & 0x8000) != 0 || htype == 0x74 {
            let mut buf4 = [0u8; 4];
            if pos + 11 > total {
                break;
            }
            read_exact_at(reader, pos + 7, &mut buf4, cancel)?;
            le_u32(&buf4) as u64
        } else {
            0
        };

        if htype == 0x74 && hsize >= 32 {
            let mut fh = vec![0u8; hsize as usize];
            read_exact_at(reader, pos, &mut fh, cancel)?;
            let mut pack_size = le_u32(&fh[7..11]) as u64;
            let method = fh[25];
            let name_len = le_u16(&fh[26..28]) as usize;
            let large = (flags & 0x0100) != 0;
            let mut name_off = 32usize;
            if large && hsize >= 40 {
                let high_pack = le_u32(&fh[32..36]) as u64;
                pack_size |= high_pack << 32;
                name_off = 40;
            }
            let raw_name = &fh[name_off..fh.len().min(name_off + name_len)];
            let name =
                String::from_utf8_lossy(raw_name.split(|&b| b == 0).next().unwrap_or(raw_name))
                    .into_owned();
            let data_offset = pos + hsize;
            // RAR4 method 0x30 ('0') is STORE. Skip directories (0xE0) and split volumes.
            let is_dir = (flags & 0x00E0) == 0x00E0;
            if method == 0x30 && !is_dir && pack_size > 0 && data_offset + pack_size <= total {
                out.push(ArchiveEntry {
                    name,
                    data_offset,
                    size: pack_size,
                    kind: ArchiveKind::Rar4,
                });
            }
            pos = data_offset + pack_size;
            continue;
        }

        if htype == 0x7B {
            break; // End of archive
        }
        pos += hsize + add_size;
    }
    Ok(out)
}

fn read_vint(buf: &[u8], idx: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    while *idx < buf.len() {
        let b = buf[*idx];
        *idx += 1;
        val |= u64::from(b & 0x7F) << shift;
        if (b & 0x80) == 0 {
            return Some(val);
        }
        shift += 7;
        if shift >= 63 {
            return None;
        }
    }
    None
}

fn list_rar5_store_entries(
    reader: &mut dyn SourceReader,
    total: u64,
    cancel: &CancellationToken,
) -> io::Result<Vec<ArchiveEntry>> {
    let mut out = Vec::new();
    let mut pos = 8u64; // Past "Rar!\x1A\x07\x01\x00"
    while pos + 8 <= total {
        // Read up to 4 KiB per block header to decode vint fields cleanly.
        let probe = (total - pos).min(4096) as usize;
        let mut buf = vec![0u8; probe];
        read_exact_at(reader, pos, &mut buf, cancel)?;
        let mut i = 4usize; // Skip CRC32
        let Some(hsize) = read_vint(&buf, &mut i) else {
            break;
        };
        let header_total = i as u64 + hsize;
        if header_total > total - pos || hsize == 0 {
            break;
        }
        if (header_total as usize) > buf.len() {
            buf.resize(header_total as usize, 0);
            read_exact_at(reader, pos, &mut buf, cancel)?;
        }
        let Some(htype) = read_vint(&buf, &mut i) else {
            break;
        };
        let Some(hflags) = read_vint(&buf, &mut i) else {
            break;
        };
        if (hflags & 0x0001) != 0 {
            let _ = read_vint(&buf, &mut i); // extra area size
        }
        let data_size = if (hflags & 0x0002) != 0 {
            read_vint(&buf, &mut i).unwrap_or(0)
        } else {
            0
        };

        if htype == 2 {
            // File header
            let file_flags = read_vint(&buf, &mut i).unwrap_or(0);
            let _unpacked_size = read_vint(&buf, &mut i).unwrap_or(0);
            let _attrs = read_vint(&buf, &mut i).unwrap_or(0);
            if (file_flags & 0x0002) != 0 && i + 4 <= buf.len() {
                i += 4; // mtime
            }
            if (file_flags & 0x0004) != 0 && i + 4 <= buf.len() {
                i += 4; // data CRC32
            }
            let comp_info = read_vint(&buf, &mut i).unwrap_or(1);
            let _host_os = read_vint(&buf, &mut i).unwrap_or(0);
            let name_len = read_vint(&buf, &mut i).unwrap_or(0) as usize;
            let comp_method = (comp_info >> 7) & 0x07;
            let is_dir = (file_flags & 0x0001) != 0;
            let name = if i + name_len <= buf.len() {
                String::from_utf8_lossy(&buf[i..i + name_len]).into_owned()
            } else {
                String::new()
            };
            let data_offset = pos + header_total;
            if comp_method == 0 && !is_dir && data_size > 0 && data_offset + data_size <= total {
                out.push(ArchiveEntry {
                    name,
                    data_offset,
                    size: data_size,
                    kind: ArchiveKind::Rar5,
                });
            }
        } else if htype == 5 {
            break; // End of archive
        }

        pos += header_total + data_size;
    }
    Ok(out)
}

/// Composable `ByteSource` exposing a single uncompressed archive entry as a
/// standalone seekable stream while forwarding cursor priority hints to the
/// underlying source.
pub struct ArchiveByteSource {
    outer: Arc<dyn ByteSource>,
    entry: ArchiveEntry,
}

impl ArchiveByteSource {
    pub fn open_largest_video(
        outer: Arc<dyn ByteSource>,
        cancel: &CancellationToken,
    ) -> io::Result<Self> {
        let entries = list_store_entries(&*outer, cancel)?;
        let entry = entries
            .iter()
            .filter(|e| is_video_name(&e.name))
            .max_by_key(|e| e.size)
            .cloned()
            .or_else(|| entries.into_iter().max_by_key(|e| e.size))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "archive contains no uncompressed video entry",
                )
            })?;
        Ok(Self { outer, entry })
    }

    pub fn from_entry(outer: Arc<dyn ByteSource>, entry: ArchiveEntry) -> Self {
        Self { outer, entry }
    }

    pub fn entry(&self) -> &ArchiveEntry {
        &self.entry
    }
}

impl ByteSource for ArchiveByteSource {
    fn size(&self) -> u64 {
        self.entry.size
    }

    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>> {
        let inner = self.outer.open_reader()?;
        Ok(Box::new(ArchiveReader {
            inner,
            base_offset: self.entry.data_offset,
            entry_size: self.entry.size,
        }))
    }

    fn describe(&self) -> String {
        format!("archive({}: {})", self.entry.name, self.outer.describe())
    }

    fn filename(&self) -> Option<&str> {
        Some(&self.entry.name)
    }

    fn available_ranges(&self) -> Option<RangeSet> {
        let outer_ranges = self.outer.available_ranges()?;
        let base = self.entry.data_offset;
        let end_limit = base.saturating_add(self.entry.size);
        let mut sliced = RangeSet::new();
        for r in outer_ranges.iter() {
            let cs = r.start.max(base);
            let ce = r.end.min(end_limit);
            if ce > cs {
                sliced.insert((cs - base)..(ce - base));
            }
        }
        Some(sliced)
    }

    fn try_read_at(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        if offset >= self.entry.size || buf.is_empty() {
            return Some(0);
        }
        let max_len = (self.entry.size - offset).min(buf.len() as u64) as usize;
        self.outer
            .try_read_at(self.entry.data_offset + offset, &mut buf[..max_len])
    }
}

struct ArchiveReader {
    inner: Box<dyn SourceReader>,
    base_offset: u64,
    entry_size: u64,
}

impl SourceReader for ArchiveReader {
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cancel: &CancellationToken,
    ) -> io::Result<usize> {
        if offset >= self.entry_size || buf.is_empty() {
            return Ok(0);
        }
        let max_len = (self.entry_size - offset).min(buf.len() as u64) as usize;
        self.inner
            .read_at(self.base_offset + offset, &mut buf[..max_len], cancel)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::MemorySource;

    fn build_store_zip(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        let lfh_offset = 0u32;
        // Local File Header
        buf.extend_from_slice(b"PK\x03\x04");
        buf.extend_from_slice(&20u16.to_le_bytes()); // version needed
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&0u16.to_le_bytes()); // method = STORE
        buf.extend_from_slice(&0u16.to_le_bytes()); // time
        buf.extend_from_slice(&0u16.to_le_bytes()); // date
        buf.extend_from_slice(&0u32.to_le_bytes()); // crc
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // extra len
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(payload);

        let cd_offset = buf.len() as u32;
        // Central Directory Header
        buf.extend_from_slice(b"PK\x01\x02");
        buf.extend_from_slice(&20u16.to_le_bytes());
        buf.extend_from_slice(&20u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // STORE
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&lfh_offset.to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
        let cd_size = (buf.len() as u32) - cd_offset;

        // EOCD
        buf.extend_from_slice(b"PK\x05\x06");
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&cd_size.to_le_bytes());
        buf.extend_from_slice(&cd_offset.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf
    }

    fn build_store_rar4(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"Rar!\x1A\x07\x00");
        // Main archive header (0x73), size 13
        buf.extend_from_slice(&[0x00, 0x00, 0x73, 0x00, 0x00, 13, 0]);
        buf.extend_from_slice(&[0u8; 6]);
        // File header (0x74), flags = 0x8000
        let hsize = (32 + name.len()) as u16;
        buf.extend_from_slice(&[0x00, 0x00, 0x74]);
        buf.extend_from_slice(&0x8000u16.to_le_bytes());
        buf.extend_from_slice(&hsize.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // pack
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // unp
        buf.push(3); // Unix
        buf.extend_from_slice(&0u32.to_le_bytes()); // crc
        buf.extend_from_slice(&0u32.to_le_bytes()); // ftime
        buf.push(29); // unpver
        buf.push(0x30); // method = STORE
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // attr
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(payload);
        // End of archive (0x7B)
        buf.extend_from_slice(&[0x00, 0x00, 0x7B, 0x00, 0x00, 7, 0]);
        buf
    }

    #[test]
    fn streams_store_zip_video_with_offsets() {
        let payload: Vec<u8> = (0..512u32).map(|i| (i % 251) as u8).collect();
        let zip = build_store_zip("movie/sample.mkv", &payload);
        let outer = Arc::new(MemorySource::new(zip.clone(), zip.len() as u64));
        let wrapped =
            ArchiveByteSource::open_largest_video(outer, &CancellationToken::new()).unwrap();
        assert_eq!(wrapped.filename(), Some("movie/sample.mkv"));
        assert_eq!(wrapped.size(), 512);

        let mut reader = wrapped.open_reader().unwrap();
        let mut out = [0u8; 64];
        let n = reader
            .read_at(100, &mut out, &CancellationToken::new())
            .unwrap();
        assert_eq!(n, 64);
        assert_eq!(&out[..], &payload[100..164]);
        assert!(wrapped.try_read_at(200, &mut out).is_some());
        assert_eq!(&out[..], &payload[200..264]);
    }

    #[test]
    fn streams_store_rar4_video() {
        let payload = b"FTYP_FAKE_MP4_PAYLOAD_0123456789";
        let rar = build_store_rar4("release.mp4", payload);
        let outer = Arc::new(MemorySource::new(rar.clone(), rar.len() as u64));
        let wrapped =
            ArchiveByteSource::open_largest_video(outer, &CancellationToken::new()).unwrap();
        assert_eq!(wrapped.filename(), Some("release.mp4"));
        assert_eq!(wrapped.size(), payload.len() as u64);
        let mut out = vec![0u8; payload.len()];
        let mut reader = wrapped.open_reader().unwrap();
        let n = reader
            .read_at(0, &mut out, &CancellationToken::new())
            .unwrap();
        assert_eq!(&out[..n], payload);
    }
}
