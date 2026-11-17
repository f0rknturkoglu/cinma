//! Finished files in the user's download folders: a `ByteSource` over a local
//! file, safe file names, and a move that never overwrites.

use std::fs::{self, File};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{ByteSource, CancellationToken, RangeSet, SourceReader, cancelled_error};

/// A complete local file as a `ByteSource` (finished downloads, casting).
pub struct FileSource {
    file: Arc<File>,
    len: u64,
    name: String,
}

impl FileSource {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_owned();
        Ok(Self {
            file: Arc::new(file),
            len,
            name,
        })
    }
}

struct FileReader {
    file: Arc<File>,
    len: u64,
}

impl SourceReader for FileReader {
    fn read_at(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        cancel: &CancellationToken,
    ) -> io::Result<usize> {
        if cancel.is_cancelled() {
            return Err(cancelled_error());
        }
        if offset >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let n = (self.len - offset).min(buf.len() as u64) as usize;
        self.file.read_at(&mut buf[..n], offset)
    }
}

impl ByteSource for FileSource {
    fn size(&self) -> u64 {
        self.len
    }

    fn open_reader(&self) -> io::Result<Box<dyn SourceReader>> {
        Ok(Box::new(FileReader {
            file: Arc::clone(&self.file),
            len: self.len,
        }))
    }

    fn describe(&self) -> String {
        format!("file:{}", self.name)
    }

    fn filename(&self) -> Option<&str> {
        Some(&self.name)
    }

    fn available_ranges(&self) -> Option<RangeSet> {
        let mut set = RangeSet::new();
        set.insert(0..self.len);
        Some(set)
    }

    fn try_read_at(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        if offset >= self.len || buf.is_empty() {
            return Some(0);
        }
        let n = (self.len - offset).min(buf.len() as u64) as usize;
        self.file.read_at(&mut buf[..n], offset).ok()
    }
}

/// Turns an untrusted title or torrent file name into one safe path
/// component: no separators, no `..`, no leading dots, no control
/// characters, bounded length. Never empty.
pub fn sanitize_file_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        let c = match c {
            '/' | '\\' | ':' => ' ',
            c if c.is_control() => ' ',
            c => c,
        };
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    let trimmed = out
        .trim()
        .trim_start_matches(['.', ' '])
        .trim_end_matches([' ', '.']);
    let mut name: String = trimmed.chars().take(180).collect();
    if name.is_empty() {
        name = "Download".to_owned();
    }
    name
}

/// `dir/name`, or `dir/stem (2).ext`, `(3)`… if taken. Never returns an
/// existing path.
pub fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 2..10_000 {
        let candidate = match ext {
            Some(ext) => dir.join(format!("{stem} ({n}).{ext}")),
            None => dir.join(format!("{stem} ({n})")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!(
        "{stem} ({}).{}",
        std::process::id(),
        ext.unwrap_or("bin")
    ))
}

/// Moves `src` to `dest` without ever overwriting `dest`: a hard link plus
/// unlink on the same volume (no data copied), else copy → fsync → rename.
pub fn move_file_no_overwrite(src: &Path, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::hard_link(src, dest) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            let mut part_name = dest.file_name().unwrap_or_default().to_os_string();
            part_name.push(".part");
            let part = dest.with_file_name(part_name);
            {
                let mut from = File::open(src)?;
                let mut to = File::create(&part)?;
                io::copy(&mut from, &mut to)?;
                to.sync_all()?;
            }
            if dest.exists() {
                let _ = fs::remove_file(&part);
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination exists",
                ));
            }
            fs::rename(&part, dest)?;
        }
        Err(e) => return Err(e),
    }
    if let Some(parent) = dest.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
    fs::remove_file(src)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_untrusted_names() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "etc passwd");
        assert_eq!(sanitize_file_name(".hidden.mkv"), "hidden.mkv");
        assert_eq!(sanitize_file_name("Show: S01E02\n.mkv"), "Show S01E02 .mkv");
        assert_eq!(sanitize_file_name("   "), "Download");
        assert_eq!(sanitize_file_name("..."), "Download");
        assert!(sanitize_file_name(&"x".repeat(500)).len() <= 180);
    }

    #[test]
    fn picks_free_names_and_moves_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.mkv"), b"keep").unwrap();
        let dest = unique_destination(dir.path(), "a.mkv");
        assert_eq!(dest, dir.path().join("a (2).mkv"));

        let src = dir.path().join("src.bin");
        fs::write(&src, b"new").unwrap();
        assert!(move_file_no_overwrite(&src, &dir.path().join("a.mkv")).is_err());
        assert_eq!(fs::read(dir.path().join("a.mkv")).unwrap(), b"keep");
        move_file_no_overwrite(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
        assert!(!src.exists());
    }

    #[test]
    fn file_source_reads_and_reports_complete() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.mkv");
        fs::write(&p, b"0123456789").unwrap();
        let src = FileSource::open(&p).unwrap();
        let mut r = src.open_reader().unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(
            r.read_at(8, &mut buf, &CancellationToken::new()).unwrap(),
            2
        );
        assert_eq!(&buf[..2], b"89");
        assert_eq!(src.available_ranges().unwrap().covered(), 10);
    }
}
