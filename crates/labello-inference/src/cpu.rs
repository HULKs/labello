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

pub(super) struct CpuSession {
    model: std::sync::Arc<TypedRunnableModel>,
    input_shape: Vec<usize>,
    output_names: Vec<Option<String>>,
}

impl CpuSession {
    pub(super) fn new(bytes: &[u8]) -> Result<Self, String> {
        let model = load(bytes)?;
        let inputs = model
            .input_outlets()
            .map_err(|_| "model inputs unavailable")?;
        let fact = model.input_fact(0).map_err(|_| "model input unavailable")?;
        let shape = fact
            .shape
            .as_concrete()
            .ok_or("model input must be static")?;
        if inputs.len() != 1 || fact.datum_type != f32::datum_type() {
            return Err("model requires one static float32 input".into());
        }
        let input_shape = shape.to_vec();
        let output_names = model
            .output_outlets()
            .map_err(|_| "model outputs unavailable")?
            .iter()
            .map(|&outlet| model.outlet_label(outlet).map(str::to_owned))
            .collect();
        let model = model
            .into_runnable()
            .map_err(|_| "model compilation failed")?;
        Ok(Self {
            model,
            input_shape,
            output_names,
        })
    }

    pub(super) fn infer(
        &self,
        image: &[u8],
        config: &PrelabelConfig,
        task: &TaskDefinition,
    ) -> Result<InferenceResult, String> {
        let spec = config.yolo.as_ref().ok_or("missing YOLO profile")?;
        let size = spec.input_size as usize;
        if self.input_shape != [1, 3, size, size] {
            return Err("model requires static float32 NCHW input; export batch=1, dynamic=false, half=false".into());
        }
        let output_index = match &spec.output_name {
            Some(name) => self
                .output_names
                .iter()
                .position(|output| output.as_ref() == Some(name))
                .ok_or("selected model output was not found; check the model again")?,
            None if self.output_names.len() == 1 => 0,
            None => return Err("select a model output tensor".into()),
        };
        let (input, letterbox) = prepare(image, spec.input_size)?;
        let tensor = Tensor::from_shape(&[1, 3, size, size], &input)
            .map_err(|_| "model tensor preparation failed")?;
        let output = self
            .model
            .run(tvec![tensor.into()])
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
}
