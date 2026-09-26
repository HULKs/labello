//! The supported Ultralytics raw ONNX tensor contract and target-specific execution.
//! Model/image acquisition, authorization, jobs and persistence belong to callers.

use image::{ImageReader, imageops::FilterType};
use labello_domain::*;
#[cfg(not(target_arch = "wasm32"))]
use ort::{
    session::Session,
    value::{Tensor, TensorElementType, ValueType},
};
use std::io::Cursor;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::infer;
#[cfg(not(target_arch = "wasm32"))]
mod inspection;
#[cfg(not(target_arch = "wasm32"))]
pub use inspection::inspect;
#[cfg(not(target_arch = "wasm32"))]
mod cpu;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{NativeProvider, NativeRuntimeConfig, configure_runtime};

pub const MAX_MODEL_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_CANDIDATES: usize = 35_000;

#[cfg(test)]
mod tests;

pub type InferenceResult = PrelabelInferenceResult;

#[derive(Clone, Copy, Debug)]
struct Letterbox {
    width: u32,
    height: u32,
    scale_x: f32,
    scale_y: f32,
    left: u32,
    top: u32,
}

fn prepare(image: &[u8], size: u32) -> Result<(Vec<f32>, Letterbox), String> {
    if image.len() > MAX_IMAGE_BYTES {
        return Err("image exceeds inference byte limit".into());
    }
    let mut reader = ImageReader::new(Cursor::new(image))
        .with_guessed_format()
        .map_err(|_| "unsupported image")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "image decode failed")?
        .to_rgb8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("empty image".into());
    }
    let ratio = (size as f32 / width as f32).min(size as f32 / height as f32);
    let resized_w = (width as f32 * ratio).round().max(1.0) as u32;
    let resized_h = (height as f32 * ratio).round().max(1.0) as u32;
    let resized = image::imageops::resize(&image, resized_w, resized_h, FilterType::Triangle);
    let left = (size - resized_w) / 2;
    let top = (size - resized_h) / 2;
    let plane = size as usize * size as usize;
    let mut data = vec![114.0 / 255.0; 3 * plane];
    for (x, y, pixel) in resized.enumerate_pixels() {
        for channel in 0..3 {
            data[channel * plane + (y + top) as usize * size as usize + (x + left) as usize] =
                pixel[channel] as f32 / 255.0;
        }
    }
    Ok((
        data,
        Letterbox {
            width,
            height,
            scale_x: resized_w as f32 / width as f32,
            scale_y: resized_h as f32 / height as f32,
            left,
            top,
        },
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_session(session: &Session, spec: &YoloModelSpec) -> Result<(), String> {
    if session.inputs().len() != 1 {
        return Err("model must have one input".into());
    }
    selected_output(session, spec)?;
    let expected = [1, 3, i64::from(spec.input_size), i64::from(spec.input_size)];
    match session.inputs()[0].dtype() {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape,
            ..
        } if shape.as_ref() == expected => Ok(()),
        _ => Err(
            "model requires static float32 NCHW input; export batch=1, dynamic=false, half=false"
                .into(),
        ),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn selected_output(session: &Session, spec: &YoloModelSpec) -> Result<usize, String> {
    match &spec.output_name {
        Some(name) => session
            .outputs()
            .iter()
            .position(|output| output.name() == name)
            .ok_or_else(|| "selected model output was not found; check the model again".into()),
        None if session.outputs().len() == 1 => Ok(0),
        None => Err("select a model output tensor".into()),
    }
}

fn decode(
    shape: &[i64],
    data: &[f32],
    letterbox: Letterbox,
    config: &PrelabelConfig,
    task: &TaskDefinition,
) -> Result<Vec<PrelabelSuggestion>, String> {
    let spec = config.yolo.as_ref().ok_or("missing YOLO profile")?;
    let channels = 4 + spec.model_class_count() + spec.keypoints.len() * 3;
    if shape.len() != 3
        || shape[0] != 1
        || shape[1] != channels as i64
        || shape[2] < 0
        || shape[2] as usize > MAX_CANDIDATES
        || data.len() > 8_000_000
        || data.len() != channels * shape[2] as usize
        || !data.iter().all(|v| v.is_finite())
    {
        return Err(
            "unsupported model output; expected raw YOLO detection/pose channels with nms=false"
                .into(),
        );
    }
    let count = shape[2] as usize;
    let x = |v: f32| {
        ((v - letterbox.left as f32) / letterbox.scale_x / letterbox.width as f32).clamp(0.0, 1.0)
    };
    let y = |v: f32| {
        ((v - letterbox.top as f32) / letterbox.scale_y / letterbox.height as f32).clamp(0.0, 1.0)
    };
    let mut hints = Vec::new();
    let mut pose_boxes = Vec::new();
    for index in 0..count {
        let at = |channel: usize| data[channel * count + index];
        let (class_index, confidence) = (0..spec.model_class_count())
            .map(|class| (class, at(4 + class)))
            .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .ok_or("missing model classes")?;
        if !(0.0..=1.0).contains(&confidence) {
            return Err("invalid model confidence".into());
        }
        let Some(class_id) = spec.dataset_class(class_index) else {
            continue;
        };
        if !task.class_ids.contains(class_id) {
            continue;
        }
        if at(2) < 0.0 || at(3) < 0.0 {
            return Err("negative model box dimensions".into());
        }
        let left = x(at(0) - at(2) / 2.0);
        let top = y(at(1) - at(3) / 2.0);
        let right = x(at(0) + at(2) / 2.0);
        let bottom = y(at(1) + at(3) / 2.0);
        if right <= left || bottom <= top {
            continue;
        }
        let geometry = if spec.keypoints.is_empty() {
            AnnotationGeometry::BoundingBox(BoundingBox {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            })
        } else {
            let mut keypoints = Vec::new();
            for (keypoint, name) in spec.keypoints.iter().enumerate() {
                let offset = 4 + spec.model_class_count() + keypoint * 3;
                let visibility = at(offset + 2);
                if !(0.0..=1.0).contains(&visibility) {
                    return Err("invalid keypoint confidence".into());
                }
                let state = if visibility >= 0.5
                    || task.skeleton.as_ref().is_some_and(|s| !s.allow_hidden)
                {
                    KeypointState::Visible
                } else {
                    KeypointState::Hidden
                };
                keypoints.push(KeypointAnnotation {
                    name: name.clone(),
                    state,
                    point: Some(NormalizedPoint {
                        x: x(at(offset)),
                        y: y(at(offset + 1)),
                    }),
                });
            }
            AnnotationGeometry::Skeleton(SkeletonGeometry { keypoints })
        };
        geometry.validate().map_err(|_| "invalid model geometry")?;
        hints.push(PrelabelSuggestion {
            suggestion_id: format!("candidate_{index}"),
            config_id: config.config_id.clone(),
            task_id: task.task_id.clone(),
            class_id: class_id.clone(),
            confidence,
            geometry,
            evidence: None,
        });
        if !spec.keypoints.is_empty() {
            let mut bounds = hints.last().expect("inserted candidate").clone();
            bounds.geometry = AnnotationGeometry::BoundingBox(BoundingBox {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            });
            pose_boxes.push(bounds);
        }
    }
    if !pose_boxes.is_empty() {
        // Pose outputs have an object box used for model NMS, independent of keypoint visibility.
        let kept: std::collections::BTreeSet<_> =
            filter_prelabels(&pose_boxes, &[], &config.output_processing)
                .into_iter()
                .map(|hint| hint.suggestion_id)
                .collect();
        hints.retain(|hint| kept.contains(&hint.suggestion_id));
    }
    // Keep box candidates for refiltering against the current editable annotation draft.
    Ok(hints)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn infer(
    model: &[u8],
    image: &[u8],
    config: &PrelabelConfig,
    task: &TaskDefinition,
) -> Result<InferenceResult, String> {
    infer_with_provider(model, image, config, task, NativeProvider::Cpu)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn infer_with_provider(
    model: &[u8],
    image: &[u8],
    config: &PrelabelConfig,
    task: &TaskDefinition,
    provider: NativeProvider,
) -> Result<InferenceResult, String> {
    config.validate_for_task(task).map_err(|e| e.to_string())?;
    if model.is_empty() || model.len() > MAX_MODEL_BYTES {
        return Err("model exceeds inference byte limit".into());
    }
    let spec = config.yolo.as_ref().ok_or("missing YOLO profile")?;
    if spec
        .model_digest
        .as_ref()
        .is_some_and(|digest| blake3::hash(model).to_hex().as_str() != digest)
    {
        return Err("model changed; check the model and mapping again".into());
    }
    if provider == NativeProvider::Cpu && !native::available() {
        return cpu::infer(model, image, config, task);
    }
    let mut native_session = native::load_session(model, provider)?;
    let session = &mut native_session.session;
    validate_session(session, spec)?;
    let output_index = selected_output(session, spec)?;
    let (input, letterbox) = prepare(image, spec.input_size)?;
    let tensor = Tensor::from_array((
        [1, 3, spec.input_size as usize, spec.input_size as usize],
        input,
    ))
    .map_err(|_| "model tensor preparation failed")?;
    let output = session
        .run(ort::inputs![tensor])
        .map_err(|_| "model execution failed")?;
    let (shape, data) = output[output_index]
        .try_extract_tensor::<f32>()
        .map_err(|_| "model output must be float32")?;
    let suggestions = decode(shape, data, letterbox, config, task)?;
    Ok(InferenceResult {
        execution: provider.execution(),
        suggestions,
    })
}
