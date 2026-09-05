use crate::{ClientError, ClientResult, EncodedImagePreview, ImagePreview};
use image::ImageDecoder;
use std::io::Cursor;

pub const MAX_ENCODED_PREVIEW_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn invalid_preview() -> ClientError {
    ClientError::Api {
        status: 0,
        message: "invalid or oversized image preview".into(),
    }
}

impl EncodedImagePreview {
    /// Decode with the same bounded Rust WebP implementation on native and WASM.
    /// Encoded bytes never occupy ImagePreview::rgba.
    pub fn decode(&self) -> ClientResult<ImagePreview> {
        if self.webp.len() > MAX_ENCODED_PREVIEW_BYTES
            || self.width == 0
            || self.height == 0
            || self.width.max(self.height) > self.profile.max_edge()
            || self.original_width == 0
            || self.original_height == 0
            || u64::from(self.original_width) * u64::from(self.original_height) > 100_000_000
            || self.width > self.original_width
            || self.height > self.original_height
        {
            return Err(invalid_preview());
        }
        let mut reader =
            image::ImageReader::with_format(Cursor::new(&self.webp), image::ImageFormat::WebP);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(self.profile.max_edge());
        limits.max_image_height = Some(self.profile.max_edge());
        limits.max_alloc = Some(32 * 1024 * 1024);
        reader.limits(limits);
        let decoder = reader.into_decoder().map_err(|_| invalid_preview())?;
        if decoder.dimensions() != (self.width, self.height)
            || decoder.total_bytes() > u64::from(self.width) * u64::from(self.height) * 4
        {
            return Err(invalid_preview());
        }
        let image = image::DynamicImage::from_decoder(decoder)
            .map_err(|_| invalid_preview())?
            .to_rgba8();
        Ok(ImagePreview {
            image_id: self.image_id.clone(),
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        })
    }
}

pub(crate) async fn bounded_body(
    response: reqwest::Response,
    limit: usize,
) -> ClientResult<Vec<u8>> {
    use futures::StreamExt;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(invalid_preview());
    }
    let mut stream = std::pin::pin!(response.bytes_stream());
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(invalid_preview());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
