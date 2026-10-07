use labello_client::{ClientError, ClientResult, EncodedImagePreview, ImagePreview};
use wasm_bindgen::{JsCast, prelude::*};

#[wasm_bindgen(inline_js = r#"
export async function decodeWorkingPreview(bytes, width, height) {
    const bitmap = await createImageBitmap(new Blob([bytes], {type: 'image/webp'}), {
        colorSpaceConversion: 'none', premultiplyAlpha: 'none', imageOrientation: 'none'
    });
    try {
        if (bitmap.width !== width || bitmap.height !== height)
            throw new Error('preview dimensions mismatch');
        const canvas = new OffscreenCanvas(width, height);
        const context = canvas.getContext('2d', {willReadFrequently: true});
        if (!context) throw new Error('preview canvas unavailable');
        context.drawImage(bitmap, 0, 0);
        return new Uint8Array(context.getImageData(0, 0, width, height).data.buffer);
    } finally {
        bitmap.close();
    }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = decodeWorkingPreview)]
    fn decode_working_preview(bytes: &[u8], width: u32, height: u32) -> js_sys::Promise;
}

pub(crate) async fn decode_browser_preview(
    encoded: EncodedImagePreview,
) -> ClientResult<ImagePreview> {
    // Validate the encoded header before the browser can allocate an image.
    // Blob copies the borrowed WASM bytes synchronously before its first await.
    encoded.validate()?;
    let invalid = || ClientError::Api {
        status: 0,
        message: "working image preview could not be decoded".into(),
    };
    let pixels = wasm_bindgen_futures::JsFuture::from(decode_working_preview(
        &encoded.webp,
        encoded.width,
        encoded.height,
    ))
    .await
    .map_err(|_| invalid())?
    .dyn_into::<js_sys::Uint8Array>()
    .map_err(|_| invalid())?;
    if u64::from(pixels.length()) != u64::from(encoded.width) * u64::from(encoded.height) * 4 {
        return Err(invalid());
    }
    Ok(ImagePreview {
        image_id: encoded.image_id,
        width: encoded.width,
        height: encoded.height,
        rgba: pixels.to_vec(),
    })
}
