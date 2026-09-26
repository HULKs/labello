use super::{InferenceResult, decode, prepare};
use labello_domain::*;
use std::io::Cursor;
use tract_onnx::prelude::*;

/// The built-in CPU runtime keeps installations without a native ORT library usable.
pub(super) fn load(model: &[u8]) -> Result<TypedModel, String> {
    tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(model))
        .and_then(|model| model.into_optimized())
        .map_err(|_| "model loading failed".into())
}

pub(super) fn infer(
    model: &[u8],
    image: &[u8],
    config: &PrelabelConfig,
    task: &TaskDefinition,
) -> Result<InferenceResult, String> {
    let spec = config.yolo.as_ref().ok_or("missing YOLO profile")?;
    let model = load(model)?;
    let inputs = model
        .input_outlets()
        .map_err(|_| "model inputs unavailable")?;
    let fact = model.input_fact(0).map_err(|_| "model input unavailable")?;
    let size = spec.input_size as usize;
    if inputs.len() != 1
        || fact.datum_type != f32::datum_type()
        || fact.shape.as_concrete() != Some(&[1, 3, size, size])
    {
        return Err(
            "model requires static float32 NCHW input; export batch=1, dynamic=false, half=false"
                .into(),
        );
    }
    let outputs = model
        .output_outlets()
        .map_err(|_| "model outputs unavailable")?;
    let output_index = match &spec.output_name {
        Some(name) => outputs
            .iter()
            .position(|&outlet| model.outlet_label(outlet) == Some(name))
            .ok_or("selected model output was not found; check the model again")?,
        None if outputs.len() == 1 => 0,
        None => return Err("select a model output tensor".into()),
    };
    let (input, letterbox) = prepare(image, spec.input_size)?;
    let tensor = Tensor::from_shape(&[1, 3, size, size], &input)
        .map_err(|_| "model tensor preparation failed")?;
    let output = model
        .into_runnable()
        .and_then(|model| model.run(tvec![tensor.into()]))
        .map_err(|_| "model execution failed")?;
    let selected = &output[output_index];
    let shape = selected
        .shape()
        .iter()
        .map(|&d| d as i64)
        .collect::<Vec<_>>();
    let plain = selected
        .try_as_plain_ram()
        .map_err(|_| "model output must be contiguous")?;
    let data = plain
        .as_slice::<f32>()
        .map_err(|_| "model output must be float32")?;
    Ok(InferenceResult {
        execution: PrelabelExecutionKind::ServerCpu,
        suggestions: decode(&shape, data, letterbox, config, task)?,
    })
}
