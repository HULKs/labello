use super::*;
use image::{ImageDecoder, ImageReader};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::{Component, Path},
};

pub(super) fn open_regular(root: &Path, relative: &Path) -> Result<File, PreviewError> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(PreviewError::Source);
    }
    #[cfg(target_os = "linux")]
    let file = {
        use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
        let directory = File::open(root).map_err(|_| PreviewError::Source)?;
        let fd = openat2(
            &directory,
            relative,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|_| PreviewError::Source)?;
        File::from(fd)
    };
    #[cfg(not(target_os = "linux"))]
    let file = {
        let mut path = root.to_path_buf();
        for part in relative.components() {
            path.push(part);
            if std::fs::symlink_metadata(&path)
                .map_err(|_| PreviewError::Source)?
                .file_type()
                .is_symlink()
            {
                return Err(PreviewError::Source);
            }
        }
        File::open(path).map_err(|_| PreviewError::Source)?
    };
    if !file.metadata().map_err(|_| PreviewError::Source)?.is_file() {
        return Err(PreviewError::Source);
    }
    Ok(file)
}

pub(super) fn source_bytes(
    root: &Path,
    record: &ImageRecord,
    config: &PreviewConfig,
) -> Result<Vec<u8>, PreviewError> {
    let source = open_regular(root, Path::new(&record.canonical_path))?;
    if source.metadata().map_err(|_| PreviewError::Source)?.len() > config.max_source_bytes {
        return Err(PreviewError::SourceLimit);
    }
    let mut bytes = Vec::new();
    source
        .take(config.max_source_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PreviewError::Source)?;
    if bytes.len() as u64 > config.max_source_bytes {
        return Err(PreviewError::SourceLimit);
    }
    if blake3::hash(&bytes).to_hex().as_str() != record.blake3 {
        return Err(PreviewError::SourceChanged);
    }
    Ok(bytes)
}

pub(super) fn resize(
    source: &[u8],
    record: &ImageRecord,
    max_edge: u32,
    config: &PreviewConfig,
) -> Result<RgbaPreview, PreviewError> {
    // Preserve image::open's extension-based decoder selection and its native
    // channel depth during Triangle resizing. EXIF/ICC are not applied.
    let format =
        image::ImageFormat::from_path(&record.canonical_path).map_err(|_| PreviewError::Source)?;
    let mut reader = ImageReader::with_format(Cursor::new(source), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(config.max_decoded_bytes);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|error| match error {
        image::ImageError::Limits(_) => PreviewError::DecoderLimit,
        _ => PreviewError::Decode,
    })?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > config.max_pixels {
        return Err(PreviewError::SourceLimit);
    }
    if decoder.total_bytes() > config.max_decoded_bytes {
        return Err(PreviewError::DecoderLimit);
    }
    if (width, height) != (record.width, record.height) {
        return Err(PreviewError::SourceChanged);
    }
    let image = image::DynamicImage::from_decoder(decoder).map_err(|error| match error {
        image::ImageError::Limits(_) => PreviewError::DecoderLimit,
        _ => PreviewError::Decode,
    })?;
    let image = if width.max(height) > max_edge {
        image.resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    Ok(RgbaPreview {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

pub(super) fn encode(
    image: &RgbaPreview,
    profile: PreviewProfile,
) -> Result<Vec<u8>, PreviewError> {
    let mut config = webp::WebPConfig::new().map_err(|_| PreviewError::Encode)?;
    config.lossless = i32::from(profile == PreviewProfile::StandardV1);
    config.quality = if profile == PreviewProfile::StandardV1 {
        75.0
    } else {
        80.0
    };
    config.exact = 1;
    config.thread_level = 0;
    let encoded = webp::Encoder::from_rgba(&image.rgba, image.width, image.height)
        .encode_advanced(&config)
        .map_err(|_| PreviewError::Encode)?;
    if encoded.len() > MAX_ENCODED_PREVIEW_BYTES {
        return Err(PreviewError::Quota);
    }
    Ok(encoded.to_vec())
}
