use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use labello_domain::{DatasetConfig, DatasetMetadata, ImageRecord, ImagesIndex};

use super::{ExportFailure, ExportLimits, archive::open_regular};

/// The root descriptor pins reads to the selected dataset, even if its path is replaced.
pub(super) struct Source {
    root: File,
    path: PathBuf,
    pub metadata: DatasetMetadata,
    pub configuration_digest: String,
    index_digest: String,
}

impl Source {
    pub fn open(path: &Path, limits: &ExportLimits) -> Result<Self, ExportFailure> {
        limits.validate()?;
        let root = File::open(path).map_err(|_| ExportFailure::Storage)?;
        let config_bytes =
            bounded_read(&root, crate::paths::DATASET_FILE, limits.max_metadata_bytes)?;
        let index_bytes = bounded_read(
            &root,
            crate::paths::IMAGES_INDEX_FILE,
            limits
                .max_metadata_bytes
                .saturating_sub(config_bytes.len() as u64),
        )?;
        let config: DatasetConfig = toml::from_str(
            std::str::from_utf8(&config_bytes).map_err(|_| ExportFailure::InvalidInput)?,
        )
        .map_err(|_| ExportFailure::InvalidInput)?;
        let index: ImagesIndex =
            serde_json::from_slice(&index_bytes).map_err(|_| ExportFailure::InvalidInput)?;
        labello_domain::validate_schema_version(config.schema_version)
            .map_err(|_| ExportFailure::InvalidInput)?;
        labello_domain::validate_schema_version(index.schema_version)
            .map_err(|_| ExportFailure::InvalidInput)?;
        if index.images_by_hash.len() > limits.max_images {
            return Err(ExportFailure::Limit);
        }
        config
            .dataset_id
            .validate_path_segment()
            .map_err(|_| ExportFailure::InvalidInput)?;
        let mut images = BTreeMap::new();
        for (hash, record) in index.images_by_hash {
            record
                .image_id
                .validate_path_segment()
                .map_err(|_| ExportFailure::InvalidInput)?;
            record
                .dimensions()
                .validate()
                .map_err(|_| ExportFailure::InvalidInput)?;
            if hash != record.blake3
                || hash.len() != 64
                || !hash
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                || images.insert(record.image_id.clone(), record).is_some()
            {
                return Err(ExportFailure::InvalidInput);
            }
        }
        Ok(Self {
            root,
            path: path.to_path_buf(),
            metadata: config.into_metadata(images),
            configuration_digest: blake3::hash(&config_bytes).to_hex().to_string(),
            index_digest: blake3::hash(&index_bytes).to_hex().to_string(),
        })
    }

    pub fn verify_configuration(&self, limits: &ExportLimits) -> Result<(), ExportFailure> {
        // Event capture uses repository paths. A replaced dataset directory cannot
        // be combined with configuration read through the earlier root descriptor.
        let current =
            std::fs::symlink_metadata(&self.path).map_err(|_| ExportFailure::SourceChanged)?;
        let captured = self.root.metadata().map_err(|_| ExportFailure::Storage)?;
        if !current.is_dir() {
            return Err(ExportFailure::SourceChanged);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if (current.dev(), current.ino()) != (captured.dev(), captured.ino()) {
                return Err(ExportFailure::SourceChanged);
            }
        }
        for (name, expected) in [
            (crate::paths::DATASET_FILE, &self.configuration_digest),
            (crate::paths::IMAGES_INDEX_FILE, &self.index_digest),
        ] {
            let bytes = bounded_read(&self.root, name, limits.max_metadata_bytes)?;
            if blake3::hash(&bytes).to_hex().as_str() != expected {
                return Err(ExportFailure::SourceChanged);
            }
        }
        Ok(())
    }

    /// The caller creates an exclusive private destination and accounts aggregate bytes.
    pub fn copy_original(
        &self,
        image: &ImageRecord,
        output: &mut File,
        limits: &ExportLimits,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportFailure> {
        self.read_original(image, output, limits, cancelled)?;
        output.sync_all().map_err(|_| ExportFailure::Storage)
    }

    /// Revalidate selected originals before publication; annotations may meanwhile advance.
    pub fn verify_original(
        &self,
        image: &ImageRecord,
        limits: &ExportLimits,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportFailure> {
        self.read_original(image, &mut std::io::sink(), limits, cancelled)
    }

    fn read_original(
        &self,
        image: &ImageRecord,
        output: &mut impl Write,
        limits: &ExportLimits,
        cancelled: &AtomicBool,
    ) -> Result<(), ExportFailure> {
        if image.byte_size > limits.max_file_bytes {
            return Err(ExportFailure::Limit);
        }
        let mut input = open_regular(&self.root, &image.canonical_path)?;
        if input.metadata().map_err(|_| ExportFailure::Storage)?.len() != image.byte_size {
            return Err(ExportFailure::SourceChanged);
        }
        let mut buffer = [0_u8; 64 * 1024];
        let mut digest = blake3::Hasher::new();
        let mut copied = 0_u64;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(ExportFailure::Cancelled);
            }
            let count = input
                .read(&mut buffer)
                .map_err(|_| ExportFailure::Storage)?;
            if count == 0 {
                break;
            }
            copied = copied
                .checked_add(count as u64)
                .ok_or(ExportFailure::Limit)?;
            if copied > image.byte_size {
                return Err(ExportFailure::SourceChanged);
            }
            digest.update(&buffer[..count]);
            output
                .write_all(&buffer[..count])
                .map_err(|_| ExportFailure::Storage)?;
        }
        if copied != image.byte_size || digest.finalize().to_hex().as_str() != image.blake3 {
            return Err(ExportFailure::SourceChanged);
        }
        Ok(())
    }
}

fn bounded_read(root: &File, name: &str, maximum: u64) -> Result<Vec<u8>, ExportFailure> {
    let input = open_regular(root, name)?;
    if input.metadata().map_err(|_| ExportFailure::Storage)?.len() > maximum {
        return Err(ExportFailure::Limit);
    }
    let mut bytes = Vec::new();
    input
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ExportFailure::Storage)?;
    if bytes.len() as u64 > maximum {
        return Err(ExportFailure::Limit);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
