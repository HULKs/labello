use super::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/browser_worker.js")]
extern "C" {
    type Run;
    #[wasm_bindgen(catch, js_name = start)]
    fn start(
        model: js_sys::Uint8Array,
        input: js_sys::Float32Array,
        size: u32,
        gpu: bool,
        runtime: &str,
        output_name: &str,
    ) -> Result<Run, JsValue>;
    #[wasm_bindgen(method, getter)]
    fn promise(this: &Run) -> js_sys::Promise;
    #[wasm_bindgen(method)]
    fn cancel(this: &Run);
}
struct RunningWorker(Run);
impl Drop for RunningWorker {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub async fn infer(
    model: &[u8],
    image: &[u8],
    config: &PrelabelConfig,
    task: &TaskDefinition,
    runtime_url: &str,
) -> Result<InferenceResult, String> {
    config.validate_for_task(task).map_err(|e| e.to_string())?;
    if model.is_empty() || model.len() > MAX_MODEL_BYTES {
        return Err("model exceeds inference byte limit".into());
    }
    let spec = config.yolo.as_ref().ok_or("missing YOLO profile")?;
    let (input, letterbox) = prepare(image, spec.input_size)?;
    let prefer_gpu = matches!(
        config.execution,
        PrelabelExecution::BrowserLocal {
            acceleration: BrowserAcceleration::WebGpuPreferred
        }
    );
    let providers: &[bool] = if prefer_gpu { &[true, false] } else { &[false] };
    for &gpu in providers {
        let worker = RunningWorker(
            start(
                js_sys::Uint8Array::from(model),
                js_sys::Float32Array::from(input.as_slice()),
                spec.input_size,
                gpu,
                runtime_url,
                spec.output_name.as_deref().unwrap_or_default(),
            )
            .map_err(|_| "browser inference worker could not start")?,
        );
        let result = wasm_bindgen_futures::JsFuture::from(worker.0.promise()).await;
        let Ok(result) = result else {
            if gpu {
                continue;
            }
            return Err("browser CPU model execution failed or timed out".into());
        };
        let shape =
            js_sys::Reflect::get(&result, &"shape".into()).map_err(|_| "invalid model output")?;
        let shape: Vec<i64> = js_sys::Array::from(&shape)
            .iter()
            .map(|v| {
                v.as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0 && v.fract() == 0.0)
                    .map(|v| v as i64)
                    .ok_or("invalid model output shape")
            })
            .collect::<Result<_, _>>()?;
        let values =
            js_sys::Reflect::get(&result, &"data".into()).map_err(|_| "invalid model output")?;
        let data = js_sys::Float32Array::new(&values);
        if data.length() > 8_000_000 {
            return Err("model output exceeds limit".into());
        }
        let execution = if gpu {
            PrelabelExecutionKind::BrowserWebGpu
        } else {
            PrelabelExecutionKind::BrowserCpu
        };
        return Ok(InferenceResult {
            execution,
            suggestions: decode(&shape, &data.to_vec(), letterbox, config, task)?,
        });
    }
    Err("browser inference unavailable".into())
}
