use super::*;
use std::collections::BTreeMap;
use tract_onnx::{pb, prelude::Framework};

mod names;
#[cfg(test)]
mod tests;

/// Reads graph declarations and metadata, then checks that the native runtime can load it.
/// The server runs this in the same bounded child process as inference, without an image.
pub fn inspect(model: &[u8]) -> Result<PrelabelModelInspection, String> {
    if model.is_empty() || model.len() > MAX_MODEL_BYTES {
        return Err("model exceeds the supported byte limit".into());
    }
    let proto = tract_onnx::onnx()
        .proto_model_for_read(&mut Cursor::new(model))
        .map_err(|_| "file is not a readable ONNX model")?;
    let graph = proto.graph.as_ref().ok_or("ONNX model has no graph")?;
    if !self_contained(graph, 0) || !proto.functions.is_empty() {
        return Err("external tensor files and local ONNX functions are unsupported".into());
    }
    if graph.input.len() > 16
        || graph.output.is_empty()
        || graph.output.len() > 64
        || proto.metadata_props.len() > 128
    {
        return Err("model declarations exceed supported limits".into());
    }
    let inputs = graph
        .input
        .iter()
        .map(tensor)
        .collect::<Result<Vec<_>, _>>()?;
    let input_size = match inputs.as_slice() {
        [input] if input.data_type == "float32" => match input.shape.as_slice() {
            [Some(1), Some(3), Some(h), Some(w)]
                if h == w && (32..=1280).contains(h) && h % 32 == 0 =>
            {
                Some(*h as u32)
            }
            _ => None,
        },
        _ => None,
    };
    let mut metadata = BTreeMap::new();
    for property in &proto.metadata_props {
        if property.key.len() > 256
            || property.value.len() > 256 * 1024
            || metadata
                .insert(property.key.as_str(), property.value.as_str())
                .is_some()
        {
            return Err("invalid model metadata".into());
        }
    }
    let keypoints = keypoint_count(&metadata);
    let class_names = class_names(&metadata);
    let mut outputs = Vec::new();
    for value in &graph.output {
        let tensor = tensor(value)?;
        let profile = (|| {
            let keypoint_count = *keypoints.as_ref().map_err(|error| *error)?;
            let names = class_names.as_ref().map_err(|error| *error)?;
            if tensor.data_type != "float32" {
                return Err("output must use float32");
            }
            let [Some(1), Some(channels), Some(candidates)] = tensor.shape.as_slice() else {
                return Err("output must have static shape [1, channels, candidates]");
            };
            let count = channels
                .checked_sub(4 + 3 * i64::from(keypoint_count))
                .ok_or("invalid output dimensions")?;
            if !(1..=1000).contains(&count)
                || !(1..=MAX_CANDIDATES as i64).contains(candidates)
                || channels * candidates > 8_000_000
            {
                return Err("unsupported raw detection or pose output; export without NMS");
            }
            if names
                .as_ref()
                .is_some_and(|names| names.len() != count as usize)
            {
                return Err("class metadata disagrees with output dimensions");
            }
            Ok(YoloOutputProfile {
                class_count: count as u32,
                class_names: names.clone().unwrap_or_default(),
                keypoint_count,
            })
        })();
        outputs.push(PrelabelModelOutput {
            tensor,
            profile: profile.as_ref().ok().cloned(),
            problem: profile.err().map(str::to_owned),
        });
    }
    let problem = if input_size.is_none() {
        Some("model requires one static float32 input [1, 3, S, S]; S must be 32..1280 and divisible by 32".into())
    } else {
        (if native::available() {
            native::load_session(model, NativeProvider::Cpu, 1).map(|_| ())
        } else {
            cpu::load(model).map(|_| ())
        })
        .err()
    };
    Ok(PrelabelModelInspection {
        model_digest: blake3::hash(model).to_hex().to_string(),
        inputs,
        input_size,
        outputs,
        problem,
    })
}

fn keypoint_count(metadata: &BTreeMap<&str, &str>) -> Result<u32, &'static str> {
    let shape = metadata.get("kpt_shape");
    let task = metadata
        .get("task")
        .map(|value| serde_json::from_str::<String>(value).unwrap_or_else(|_| (*value).to_owned()));
    match task.as_deref() {
        None | Some("detect") if shape.is_none() => Ok(0),
        None | Some("pose") => shape
            .and_then(|value| serde_json::from_str::<Vec<u32>>(value).ok())
            .filter(|shape| matches!(shape.as_slice(), [count, 3] if (1..=256).contains(count)))
            .map(|shape| shape[0])
            .ok_or("pose metadata must declare [keypoints, 3]"),
        Some("detect") => Err("detection metadata must not declare keypoints"),
        Some(_) => Err("unsupported model task; expected detection or pose"),
    }
}

fn class_names(metadata: &BTreeMap<&str, &str>) -> Result<Option<Vec<String>>, &'static str> {
    let parse = |key| {
        metadata
            .get(key)
            .map(|value| names::parse(value))
            .transpose()
            .map_err(|_| "invalid model class-name metadata")
    };
    let names = parse("names")?;
    let classes = parse("classes")?;
    if let (Some(names), Some(classes)) = (&names, &classes)
        && names != classes
    {
        return Err("names and classes metadata disagree");
    }
    Ok(names.or(classes))
}

fn tensor(value: &pb::ValueInfoProto) -> Result<PrelabelTensor, String> {
    if value.name.is_empty() || value.name.len() > 256 {
        return Err("invalid tensor name".into());
    }
    let Some(pb::type_proto::Value::TensorType(kind)) =
        value.r#type.as_ref().and_then(|t| t.value.as_ref())
    else {
        return Ok(PrelabelTensor {
            name: value.name.clone(),
            data_type: "non-tensor".into(),
            shape: vec![],
        });
    };
    let shape = kind
        .shape
        .as_ref()
        .map(|shape| {
            shape
                .dim
                .iter()
                .map(|dim| match dim.value {
                    Some(pb::tensor_shape_proto::dimension::Value::DimValue(value)) => Some(value),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if shape.len() > 16 {
        return Err("tensor rank exceeds supported limits".into());
    }
    let data_type = match kind.elem_type {
        1 => "float32",
        10 => "float16",
        11 => "float64",
        6 => "int32",
        7 => "int64",
        _ => "unsupported",
    };
    Ok(PrelabelTensor {
        name: value.name.clone(),
        data_type: data_type.into(),
        shape,
    })
}

fn internal(tensor: &pb::TensorProto) -> bool {
    tensor.external_data.is_empty() && tensor.data_location != Some(1)
}

fn self_contained(graph: &pb::GraphProto, depth: usize) -> bool {
    depth < 32
        && graph.initializer.iter().all(internal)
        && graph.sparse_initializer.iter().all(|t| {
            t.values.as_ref().is_none_or(internal) && t.indices.as_ref().is_none_or(internal)
        })
        && graph.node.iter().flat_map(|node| &node.attribute).all(|a| {
            a.t.as_ref().is_none_or(internal)
                && a.tensors.iter().all(internal)
                && a.g.as_ref().is_none_or(|g| self_contained(g, depth + 1))
                && a.graphs.iter().all(|g| self_contained(g, depth + 1))
                && a.sparse_tensor.as_ref().is_none_or(|t| {
                    t.values.as_ref().is_none_or(internal)
                        && t.indices.as_ref().is_none_or(internal)
                })
                && a.sparse_tensors.iter().all(|t| {
                    t.values.as_ref().is_none_or(internal)
                        && t.indices.as_ref().is_none_or(internal)
                })
        })
}
