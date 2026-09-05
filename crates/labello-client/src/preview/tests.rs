use super::*;
use crate::ImagePreviewProfile;

fn valid() -> EncodedImagePreview {
    EncodedImagePreview {
        image_id: "image".into(),
        profile: ImagePreviewProfile::StandardV1,
        width: 1,
        height: 1,
        original_width: 1,
        original_height: 1,
        webp: include_bytes!("../demo/fixtures/standard.webp").to_vec(),
    }
}

#[test]
fn encoded_preview_decodes_to_real_rgba_and_rejects_untrusted_metadata() {
    let preview = valid();
    let decoded = preview.decode().unwrap();
    assert_eq!(decoded.rgba, [18, 23, 34, 255]);
    assert_eq!(decoded.image_id, preview.image_id);
    for mutate in [
        |p: &mut EncodedImagePreview| p.width = 0,
        |p: &mut EncodedImagePreview| p.width = 1601,
        |p: &mut EncodedImagePreview| p.original_width = 0,
        |p: &mut EncodedImagePreview| p.original_width = u32::MAX,
        |p: &mut EncodedImagePreview| {
            p.width = 2;
            p.original_width = 2;
        },
        |p: &mut EncodedImagePreview| p.webp = b"invalid private content".to_vec(),
        |p: &mut EncodedImagePreview| p.webp.resize(MAX_ENCODED_PREVIEW_BYTES + 1, 0),
    ] {
        let mut preview = valid();
        mutate(&mut preview);
        let error = preview.decode().unwrap_err().to_string();
        assert!(error.contains("invalid or oversized image preview"));
        assert!(!error.contains("private content"));
    }
}

#[test]
fn data_saver_uses_the_same_bounded_decoder() {
    let mut preview = valid();
    preview.profile = ImagePreviewProfile::DataSaverV1;
    preview.webp = include_bytes!("../demo/fixtures/data-saver.webp").to_vec();
    assert_eq!(preview.decode().unwrap().rgba.len(), 4);
    preview.width = 1281;
    preview.original_width = 1281;
    assert!(preview.decode().is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn streaming_body_rejects_declared_and_chunked_overflow() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    for response in [
        "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\n012\r\n3\r\n345\r\n0\r\n\r\n",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 2048];
            assert!(socket.read(&mut buffer).await.unwrap() > 0);
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let response = reqwest::get(format!("http://{address}")).await.unwrap();
        assert!(bounded_body(response, 5).await.is_err());
        server.await.unwrap();
    }
}

#[test]
fn original_detail_decoder_preserves_pixels_and_rejects_mismatched_or_oversized_metadata() {
    let file = crate::ImageFile {
        image_id: "image".into(),
        media_type: "image/webp".into(),
        bytes: valid().webp,
    };
    assert_eq!(
        file.decode_original_detail(1, 1).unwrap().rgba,
        [18, 23, 34, 255]
    );
    for (width, height) in [(0, 1), (2, 1), (u32::MAX, 1), (8000, 8000)] {
        assert!(file.decode_original_detail(width, height).is_err());
    }
    let mut invalid = file.clone();
    invalid.media_type = "application/octet-stream".into();
    assert!(invalid.decode_original_detail(1, 1).is_err());
    invalid.media_type = "image/png".into();
    assert!(invalid.decode_original_detail(1, 1).is_err());
}
