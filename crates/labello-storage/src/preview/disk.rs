use super::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAGIC: &[u8; 8] = b"LABPRV01";
const MAX_HEADER: usize = 2048;

#[derive(Default)]
pub(super) struct CacheState {
    process_lock: Option<File>,
    entries: BTreeMap<String, Entry>,
    clock: u64,
    bytes: u64,
}
struct Entry {
    bytes: u64,
    used: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    key: String,
    digest: String,
    profile: PreviewProfile,
    width: u32,
    height: u32,
    original_width: u32,
    original_height: u32,
}

impl CacheState {
    pub(super) fn initialize(
        &mut self,
        root: &Path,
        config: &PreviewConfig,
    ) -> Result<(), PreviewError> {
        if self.process_lock.is_some() {
            return Ok(());
        }
        fs::create_dir_all(root).map_err(|_| PreviewError::Cache)?;
        if fs::symlink_metadata(root)
            .map_err(|_| PreviewError::Cache)?
            .file_type()
            .is_symlink()
        {
            return Err(PreviewError::Cache);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))
                .map_err(|_| PreviewError::Cache)?;
        }
        let lock_path = root.join("cache.lock");
        if fs::symlink_metadata(&lock_path).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(PreviewError::Cache);
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| PreviewError::Cache)?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| PreviewError::Busy)?;
        let mut found = Vec::new();
        for (index, entry) in fs::read_dir(root)
            .map_err(|_| PreviewError::Cache)?
            .enumerate()
        {
            if index > 32_768 {
                return Err(PreviewError::Cache);
            }
            let entry = entry.map_err(|_| PreviewError::Cache)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| PreviewError::Cache)?;
            if name == "cache.lock" {
                continue;
            }
            if let Some(id) = name
                .strip_prefix(".preview-")
                .and_then(|name| name.strip_suffix(".tmp"))
                && uuid::Uuid::parse_str(id).is_ok()
            {
                fs::remove_file(entry.path()).map_err(|_| PreviewError::Cache)?;
                continue;
            }
            let Some(key) = name.strip_suffix(".preview").filter(|key| {
                key.len() == 64
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            }) else {
                return Err(PreviewError::Cache);
            };
            let metadata = fs::symlink_metadata(entry.path()).map_err(|_| PreviewError::Cache)?;
            if !metadata.is_file() {
                return Err(PreviewError::Cache);
            }
            found.push((
                metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                key.to_string(),
                metadata.len(),
            ));
        }
        found.sort();
        self.entries.clear();
        self.bytes = 0;
        self.clock = 0;
        for (_, key, size) in found {
            self.clock += 1;
            self.bytes = self.bytes.checked_add(size).ok_or(PreviewError::Cache)?;
            self.entries.insert(
                key,
                Entry {
                    bytes: size,
                    used: self.clock,
                },
            );
        }
        self.evict(root, config, 0, 0)?;
        self.process_lock = Some(lock);
        Ok(())
    }

    fn evict(
        &mut self,
        root: &Path,
        config: &PreviewConfig,
        incoming_bytes: u64,
        incoming_entries: usize,
    ) -> Result<(), PreviewError> {
        if incoming_bytes > config.cache_bytes {
            return Err(PreviewError::Quota);
        }
        while self.bytes.saturating_add(incoming_bytes) > config.cache_bytes
            || self.entries.len() + incoming_entries > config.cache_entries
        {
            let key = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
                .ok_or(PreviewError::Quota)?;
            self.remove(root, &key)?;
        }
        Ok(())
    }

    fn remove(&mut self, root: &Path, key: &str) -> Result<(), PreviewError> {
        match fs::remove_file(root.join(format!("{key}.preview"))) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(PreviewError::Cache),
        }
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
        }
        Ok(())
    }

    pub(super) fn read(
        &mut self,
        root: &Path,
        key: &str,
        profile: PreviewProfile,
        record: &ImageRecord,
    ) -> Result<Option<EncodedPreview>, PreviewError> {
        if !self.entries.contains_key(key) {
            return Ok(None);
        }
        match read_entry(root, key, profile, record) {
            Ok(preview) => {
                self.clock += 1;
                self.entries.get_mut(key).unwrap().used = self.clock;
                Ok(Some(preview))
            }
            Err(_) => {
                self.remove(root, key)?;
                Ok(None)
            }
        }
    }

    pub(super) fn publish(
        &mut self,
        root: &Path,
        config: &PreviewConfig,
        key: &str,
        preview: &EncodedPreview,
    ) -> Result<(), PreviewError> {
        let header = Header {
            key: key.into(),
            digest: blake3::hash(&preview.webp).to_hex().to_string(),
            profile: preview.profile,
            width: preview.width,
            height: preview.height,
            original_width: preview.original_width,
            original_height: preview.original_height,
        };
        let header = serde_json::to_vec(&header).map_err(|_| PreviewError::Encode)?;
        if header.len() > MAX_HEADER {
            return Err(PreviewError::Encode);
        }
        let bytes = (MAGIC.len() + 4 + header.len() + preview.webp.len()) as u64;
        self.evict(root, config, bytes, 1)?;
        let temporary = root.join(format!(".preview-{}.tmp", uuid::Uuid::new_v4()));
        let destination = root.join(format!("{key}.preview"));
        let write = || -> std::io::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(MAGIC)?;
            file.write_all(&(header.len() as u32).to_le_bytes())?;
            file.write_all(&header)?;
            file.write_all(&preview.webp)?;
            file.sync_all()?;
            fs::rename(&temporary, &destination)?;
            File::open(root)?.sync_all()
        };
        if write().is_err() {
            let _ = fs::remove_file(&temporary);
            let _ = fs::remove_file(&destination);
            return Err(PreviewError::Cache);
        }
        self.clock += 1;
        self.bytes += bytes;
        self.entries.insert(
            key.into(),
            Entry {
                bytes,
                used: self.clock,
            },
        );
        Ok(())
    }
}

fn read_entry(
    root: &Path,
    key: &str,
    profile: PreviewProfile,
    record: &ImageRecord,
) -> Result<EncodedPreview, PreviewError> {
    let mut file = codec::open_regular(root, Path::new(&format!("{key}.preview")))?;
    if file.metadata().map_err(|_| PreviewError::Cache)?.len()
        > (MAX_ENCODED_PREVIEW_BYTES + MAX_HEADER + 12) as u64
    {
        return Err(PreviewError::Cache);
    }
    let mut prefix = [0; 12];
    file.read_exact(&mut prefix)
        .map_err(|_| PreviewError::Cache)?;
    if &prefix[..8] != MAGIC {
        return Err(PreviewError::Cache);
    }
    let size = u32::from_le_bytes(prefix[8..].try_into().unwrap()) as usize;
    if size > MAX_HEADER {
        return Err(PreviewError::Cache);
    }
    let mut header = vec![0; size];
    file.read_exact(&mut header)
        .map_err(|_| PreviewError::Cache)?;
    let header: Header = serde_json::from_slice(&header).map_err(|_| PreviewError::Cache)?;
    if header.key != key
        || header.profile != profile
        || header.original_width != record.width
        || header.original_height != record.height
        || header.width == 0
        || header.height == 0
        || header.width.max(header.height) > profile.max_edge()
    {
        return Err(PreviewError::Cache);
    }
    let mut bytes = Vec::new();
    file.take((MAX_ENCODED_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| PreviewError::Cache)?;
    if bytes.len() > MAX_ENCODED_PREVIEW_BYTES
        || blake3::hash(&bytes).to_hex().as_str() != header.digest
    {
        return Err(PreviewError::Cache);
    }
    let reader =
        image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::WebP);
    if reader.into_dimensions().map_err(|_| PreviewError::Cache)? != (header.width, header.height) {
        return Err(PreviewError::Cache);
    }
    Ok(EncodedPreview {
        profile,
        width: header.width,
        height: header.height,
        original_width: header.original_width,
        original_height: header.original_height,
        webp: bytes,
    })
}
