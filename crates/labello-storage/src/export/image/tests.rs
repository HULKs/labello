use super::*;
use labello_domain::ImageId;
use std::io::{Cursor, Write};

fn bytes(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        height,
        image::Rgb([40, 120, 230]),
    ));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn record(bytes: &[u8], width: u32, height: u32) -> ImageRecord {
    ImageRecord {
        image_id: ImageId::from("image"),
        blake3: blake3::hash(bytes).to_hex().to_string(),
        canonical_path: "images/original.png".into(),
        known_paths: vec![],
        duplicate_paths: vec![],
        file_name: "original.png".into(),
        byte_size: bytes.len() as u64,
        width,
        height,
        media_type: "image/png".into(),
        source_memberships: None,
    }
}

fn check(
    bytes: &[u8],
    record: &ImageRecord,
    limits: &ExportLimits,
) -> Result<&'static str, ExportFailure> {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(bytes).unwrap();
    validate(&mut file, record, limits)
}

#[test]
fn supported_static_originals_keep_their_format_and_dimensions() {
    for (format, extension) in [
        (ImageFormat::Png, "png"),
        (ImageFormat::Jpeg, "jpg"),
        (ImageFormat::WebP, "webp"),
        (ImageFormat::Bmp, "bmp"),
    ] {
        let bytes = bytes(format, 16, 12);
        assert_eq!(
            check(&bytes, &record(&bytes, 16, 12), &ExportLimits::default()),
            Ok(extension)
        );
    }
}

#[test]
fn invalid_small_changed_and_excessive_images_cannot_be_published() {
    let data = bytes(ImageFormat::Png, 16, 12);
    let image = record(&data, 16, 12);
    assert_eq!(
        check(&data, &record(&data, 17, 12), &ExportLimits::default()),
        Err(ExportFailure::SourceChanged)
    );
    assert_eq!(
        check(&data, &record(&data, 9, 12), &ExportLimits::default()),
        Err(ExportFailure::UnsupportedImage)
    );
    assert!(
        check(
            &data,
            &image,
            &ExportLimits {
                max_decoded_image_bytes: 32,
                ..ExportLimits::default()
            }
        )
        .is_err()
    );
    assert!(check(&data[..data.len() / 2], &image, &ExportLimits::default()).is_err());
    let gif = bytes(ImageFormat::Gif, 16, 12);
    assert_eq!(
        check(&gif, &record(&gif, 16, 12), &ExportLimits::default()),
        Err(ExportFailure::UnsupportedImage)
    );
}

#[test]
fn nonidentity_exif_orientation_is_rejected_without_rotating_originals() {
    let original = bytes(ImageFormat::Jpeg, 16, 12);
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut oriented = vec![0xff, 0xd8, 0xff, 0xe1];
    oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    oriented.extend_from_slice(exif);
    oriented.extend_from_slice(&original[2..]);
    assert_eq!(
        check(
            &oriented,
            &record(&oriented, 16, 12),
            &ExportLimits::default()
        ),
        Err(ExportFailure::UnsupportedImage)
    );
}
