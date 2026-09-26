use super::*;
use std::{fs::File, io::Read};

pub(super) fn ensure_private_directory(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(PrelabelFailure::Storage),
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| PrelabelFailure::Storage)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PrelabelFailure::Storage);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn private_root(datasets_root: &Path) -> Result<(File, PathBuf)> {
    use rustix::fs::{Mode, OFlags, ResolveFlags, mkdirat, openat2};
    use std::os::fd::AsRawFd;
    let mut root = File::open(datasets_root).map_err(|_| PrelabelFailure::Storage)?;
    for name in [".labello-server", "prelabels"] {
        match mkdirat(&root, name, Mode::from_bits_truncate(0o700)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(_) => return Err(PrelabelFailure::Storage),
        }
        root = File::from(
            openat2(
                &root,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
            )
            .map_err(|_| PrelabelFailure::Storage)?,
        );
    }
    use std::os::unix::fs::PermissionsExt;
    root.set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|_| PrelabelFailure::Storage)?;
    let path = PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()));
    Ok((root, path))
}
#[cfg(not(target_os = "linux"))]
pub(super) fn private_root(_: &Path) -> Result<(File, PathBuf)> {
    Err(PrelabelFailure::Invalid)
}

#[cfg(target_os = "linux")]
pub(super) fn read_beneath(root: &File, path: &Path, limit: usize) -> Result<Vec<u8>> {
    use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
    let file = File::from(
        openat2(
            root,
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|_| PrelabelFailure::Invalid)?,
    );
    let metadata = file.metadata().map_err(|_| PrelabelFailure::Storage)?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(PrelabelFailure::Limit);
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PrelabelFailure::Storage)?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err(PrelabelFailure::Limit);
    }
    Ok(bytes)
}
#[cfg(not(target_os = "linux"))]
pub(super) fn read_beneath(_: &File, _: &Path, _: usize) -> Result<Vec<u8>> {
    Err(PrelabelFailure::Invalid)
}

pub(super) fn image(repo: &DatasetRepository, record: &ImageRecord) -> Result<Vec<u8>> {
    let root = File::open(repo.root()).map_err(|_| PrelabelFailure::Storage)?;
    let bytes = read_beneath(&root, Path::new(&record.canonical_path), 32 * 1024 * 1024)?;
    if blake3::hash(&bytes).to_hex().as_str() != record.blake3 {
        return Err(PrelabelFailure::Stale);
    }
    Ok(bytes)
}
