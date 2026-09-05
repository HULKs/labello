use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::{ExportFailure, ExportLimits};

const BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CapturedFile {
    pub path: String,
    pub bytes: u64,
    pub blake3: String,
}

pub(super) fn check_entry(path: &str) -> Result<(), ExportFailure> {
    if path.is_empty()
        || path.len() > 240
        || path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || part.ends_with('.')
                || reserved_name(part)
        })
    {
        return Err(ExportFailure::InvalidInput);
    }
    Ok(())
}

fn reserved_name(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        })
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), ExportFailure> {
    if cancel.load(Ordering::Acquire) {
        Err(ExportFailure::Cancelled)
    } else {
        Ok(())
    }
}

/// Descriptor-relative reads refuse symlinks in every component, including a replaced parent.
#[cfg(target_os = "linux")]
pub(crate) fn open_regular(root: &File, relative: &str) -> Result<File, ExportFailure> {
    use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
    if relative.is_empty()
        || Path::new(relative)
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(ExportFailure::InvalidInput);
    }
    let fd = openat2(
        root,
        relative,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|_| ExportFailure::InvalidInput)?;
    let file = File::from(fd);
    if !file
        .metadata()
        .map_err(|_| ExportFailure::Storage)?
        .is_file()
    {
        return Err(ExportFailure::InvalidInput);
    }
    Ok(file)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn open_regular(_root: &File, _relative: &str) -> Result<File, ExportFailure> {
    Err(ExportFailure::InvalidInput)
}

pub(super) fn build(
    root: &Path,
    output: File,
    files: &[CapturedFile],
    limits: &ExportLimits,
    cancel: &AtomicBool,
) -> Result<File, ExportFailure> {
    limits.validate()?;
    if files.len() > limits.max_files {
        return Err(ExportFailure::Limit);
    }
    let root = File::open(root).map_err(|_| ExportFailure::Storage)?;
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for file in files {
        check_entry(&file.path)?;
        if !names.insert(file.path.to_ascii_lowercase()) {
            return Err(ExportFailure::InvalidInput);
        }
        if file.bytes > limits.max_file_bytes {
            return Err(ExportFailure::Limit);
        }
        total = total.checked_add(file.bytes).ok_or(ExportFailure::Limit)?;
    }
    // Stored members need only bounded ZIP headers plus central directory records.
    let overhead = u64::try_from(files.len()).map_err(|_| ExportFailure::Limit)? * 1024 + 1024;
    if total
        .checked_add(overhead)
        .is_none_or(|bytes| bytes > limits.max_archive_bytes)
    {
        return Err(ExportFailure::Limit);
    }
    let mut zip = ZipWriter::new(BoundedFile {
        inner: output,
        maximum: limits.max_archive_bytes,
    });
    let mut buffer = [0_u8; BUFFER_BYTES];
    for entry in files {
        check_cancel(cancel)?;
        let mut source = open_regular(&root, &entry.path)?;
        if source.metadata().map_err(|_| ExportFailure::Storage)?.len() != entry.bytes {
            return Err(ExportFailure::SourceChanged);
        }
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(entry.bytes >= u64::from(u32::MAX))
            .unix_permissions(0o600);
        zip.start_file(&entry.path, options)
            .map_err(|_| ExportFailure::Storage)?;
        let mut digest = blake3::Hasher::new();
        let mut copied = 0_u64;
        loop {
            check_cancel(cancel)?;
            let count = source
                .read(&mut buffer)
                .map_err(|_| ExportFailure::Storage)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > entry.bytes {
                return Err(ExportFailure::SourceChanged);
            }
            digest.update(&buffer[..count]);
            zip.write_all(&buffer[..count])
                .map_err(|_| ExportFailure::Storage)?;
        }
        if copied != entry.bytes || digest.finalize().to_hex().as_str() != entry.blake3 {
            return Err(ExportFailure::SourceChanged);
        }
    }
    let mut file = zip.finish().map_err(|_| ExportFailure::Storage)?.inner;
    file.sync_all().map_err(|_| ExportFailure::Storage)?;
    file.rewind().map_err(|_| ExportFailure::Storage)?;
    verify(&mut file, files, cancel)?;
    file.rewind().map_err(|_| ExportFailure::Storage)?;
    Ok(file)
}

fn verify(
    file: &mut File,
    expected: &[CapturedFile],
    cancel: &AtomicBool,
) -> Result<(), ExportFailure> {
    let mut archive = ZipArchive::new(file).map_err(|_| ExportFailure::Verification)?;
    if archive.len() != expected.len() {
        return Err(ExportFailure::Verification);
    }
    let mut buffer = [0_u8; BUFFER_BYTES];
    for (index, expected) in expected.iter().enumerate() {
        check_cancel(cancel)?;
        let mut entry = archive
            .by_index(index)
            .map_err(|_| ExportFailure::Verification)?;
        if entry.name() != expected.path || entry.size() != expected.bytes || !entry.is_file() {
            return Err(ExportFailure::Verification);
        }
        let mut digest = blake3::Hasher::new();
        let mut bytes = 0_u64;
        loop {
            check_cancel(cancel)?;
            let count = entry
                .read(&mut buffer)
                .map_err(|_| ExportFailure::Verification)?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > expected.bytes {
                return Err(ExportFailure::Verification);
            }
            digest.update(&buffer[..count]);
        }
        if bytes != expected.bytes || digest.finalize().to_hex().as_str() != expected.blake3 {
            return Err(ExportFailure::Verification);
        }
    }
    Ok(())
}

struct BoundedFile {
    inner: File,
    maximum: u64,
}

impl Write for BoundedFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .inner
            .stream_position()?
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.maximum)
        {
            return Err(std::io::Error::other("export archive limit"));
        }
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl Seek for BoundedFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(root: &Path) -> Vec<CapturedFile> {
        std::fs::create_dir_all(root.join("labels/train")).unwrap();
        [
            ("data.yaml", b"names: [person]\n".as_slice()),
            ("labels/train/image.txt", b"".as_slice()),
        ]
        .into_iter()
        .map(|(path, bytes)| {
            std::fs::write(root.join(path), bytes).unwrap();
            CapturedFile {
                path: path.into(),
                bytes: bytes.len() as u64,
                blake3: blake3::hash(bytes).to_hex().to_string(),
            }
        })
        .collect()
    }

    fn output() -> File {
        tempfile::tempfile().unwrap()
    }

    #[test]
    fn verified_archive_retains_payload_and_explicit_empty_labels() {
        let root = tempfile::tempdir().unwrap();
        let files = fixture(root.path());
        let file = build(
            root.path(),
            output(),
            &files,
            &ExportLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let mut zip = ZipArchive::new(file).unwrap();
        assert_eq!(zip.len(), 2);
        let mut text = String::new();
        zip.by_name("data.yaml")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "names: [person]\n");
        assert_eq!(zip.by_name("labels/train/image.txt").unwrap().size(), 0);
    }

    #[test]
    fn archive_refuses_changed_capture_cancelled_jobs_and_limits() {
        let root = tempfile::tempdir().unwrap();
        let files = fixture(root.path());
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            build(
                root.path(),
                output(),
                &files,
                &ExportLimits::default(),
                &cancelled
            )
            .unwrap_err(),
            ExportFailure::Cancelled
        );
        let limits = ExportLimits {
            max_archive_bytes: 20,
            max_source_bytes: 20,
            max_file_bytes: 20,
            ..ExportLimits::default()
        };
        assert_eq!(
            build(
                root.path(),
                output(),
                &files,
                &limits,
                &AtomicBool::new(false)
            )
            .unwrap_err(),
            ExportFailure::Limit
        );
        std::fs::write(root.path().join("data.yaml"), b"names: [object]\n").unwrap();
        assert_eq!(
            build(
                root.path(),
                output(),
                &files,
                &ExportLimits::default(),
                &AtomicBool::new(false)
            )
            .unwrap_err(),
            ExportFailure::SourceChanged
        );
    }

    #[test]
    fn archive_paths_are_portable_and_case_insensitive_collisions_are_rejected() {
        for path in [
            "../outside",
            "/absolute",
            "a/../b",
            "a//b",
            "a\\b",
            "a:stream",
            "con.txt",
            "Lpt1",
            "a./b",
            "a b",
        ] {
            assert!(check_entry(path).is_err(), "{path}");
        }
        assert!(check_entry("images/train/img_0123.png").is_ok());
        let root = tempfile::tempdir().unwrap();
        let mut files = fixture(root.path());
        files.push(CapturedFile {
            path: "DATA.yaml".into(),
            ..files[0].clone()
        });
        assert_eq!(
            build(
                root.path(),
                output(),
                &files,
                &ExportLimits::default(),
                &AtomicBool::new(false)
            )
            .unwrap_err(),
            ExportFailure::InvalidInput
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn secure_reads_refuse_symlink_components_and_nonregular_files() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("private"), b"private").unwrap();
        symlink(outside.path(), root.path().join("link")).unwrap();
        symlink(outside.path().join("private"), root.path().join("file")).unwrap();
        let directory = File::open(root.path()).unwrap();
        for path in ["link/private", "file", "../outside", "."] {
            assert_eq!(
                open_regular(&directory, path).unwrap_err(),
                ExportFailure::InvalidInput
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn exhausted_storage_is_a_bounded_failure() {
        let root = tempfile::tempdir().unwrap();
        let files = fixture(root.path());
        let full = File::options().write(true).open("/dev/full").unwrap();
        assert_eq!(
            build(
                root.path(),
                full,
                &files,
                &ExportLimits::default(),
                &AtomicBool::new(false)
            )
            .unwrap_err(),
            ExportFailure::Storage
        );
    }
}
