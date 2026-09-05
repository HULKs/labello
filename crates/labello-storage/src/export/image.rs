use std::{
    fs::File,
    io::{BufReader, Seek, SeekFrom},
};

use image::{ImageDecoder, ImageFormat, ImageReader, metadata::Orientation};
use labello_domain::ImageRecord;

use super::{ExportFailure, ExportLimits};

/// Validate the private captured original, preserving its coordinate convention.
/// No orientation normalization, transcoding or first-frame extraction is allowed.
pub(super) fn validate(
    file: &mut File,
    record: &ImageRecord,
    limits: &ExportLimits,
) -> Result<&'static str, ExportFailure> {
    if record.width < 10 || record.height < 10 {
        // The pinned Ultralytics reader rejects images smaller than ten pixels.
        return Err(ExportFailure::UnsupportedImage);
    }
    file.rewind().map_err(|_| ExportFailure::Storage)?;
    let mut reader = ImageReader::new(BufReader::new(
        file.try_clone().map_err(|_| ExportFailure::Storage)?,
    ))
    .with_guessed_format()
    .map_err(|_| ExportFailure::UnsupportedImage)?;
    let format = reader.format().ok_or(ExportFailure::UnsupportedImage)?;
    let extension = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => "webp",
        ImageFormat::Bmp => "bmp",
        _ => return Err(ExportFailure::UnsupportedImage),
    };
    let mut decoder_limits = image::Limits::default();
    decoder_limits.max_alloc = Some(limits.max_decoded_image_bytes);
    reader.limits(decoder_limits.clone());
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| ExportFailure::UnsupportedImage)?;
    decoder
        .set_limits(decoder_limits.clone())
        .map_err(|_| ExportFailure::Limit)?;
    if decoder.dimensions() != (record.width, record.height) {
        return Err(ExportFailure::SourceChanged);
    }
    if decoder.total_bytes() > limits.max_decoded_image_bytes {
        return Err(ExportFailure::Limit);
    }
    if decoder
        .orientation()
        .map_err(|_| ExportFailure::UnsupportedImage)?
        != Orientation::NoTransforms
    {
        return Err(ExportFailure::UnsupportedImage);
    }
    // Fully decode with the same finite allocation limit, so truncated payloads
    // cannot be published merely because their image header is readable.
    image::DynamicImage::from_decoder(decoder).map_err(|_| ExportFailure::UnsupportedImage)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ExportFailure::Storage)?;
    let input = BufReader::new(file.try_clone().map_err(|_| ExportFailure::Storage)?);
    let animated = match format {
        ImageFormat::Png => {
            let mut decoder = image::codecs::png::PngDecoder::new(input)
                .map_err(|_| ExportFailure::UnsupportedImage)?;
            decoder
                .set_limits(decoder_limits)
                .map_err(|_| ExportFailure::Limit)?;
            decoder
                .is_apng()
                .map_err(|_| ExportFailure::UnsupportedImage)?
        }
        ImageFormat::WebP => {
            let mut decoder = image::codecs::webp::WebPDecoder::new(input)
                .map_err(|_| ExportFailure::UnsupportedImage)?;
            decoder
                .set_limits(decoder_limits)
                .map_err(|_| ExportFailure::Limit)?;
            decoder.has_animation()
        }
        _ => false,
    };
    if animated {
        return Err(ExportFailure::UnsupportedImage);
    }
    Ok(extension)
}

#[cfg(test)]
mod tests;
