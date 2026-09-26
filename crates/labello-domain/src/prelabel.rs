use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationGeometry, AnnotationVersion, ClassId, DatasetId, DomainError, DomainResult, ImageId,
    PrelabelConfigId, TaskDefinition, TaskId,
};

mod filtering;
pub use filtering::filter_prelabels;
mod management;
pub use management::*;
mod model;
pub use model::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelConfig {
    pub config_id: PrelabelConfigId,
    pub name: String,
    pub model: ModelSpec,
    pub execution: PrelabelExecution,
    pub output_processing: OutputProcessing,
    pub available_to_annotators: bool,
    /// Missing on historical configuration. Such configurations cannot execute a model.
    #[serde(default)]
    pub yolo: Option<YoloModelSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    pub model_id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub location: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAcceleration {
    WebGpuPreferred,
    WasmCpuFallback,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PrelabelExecution {
    ServerSide { command: Vec<String> },
    BrowserLocal { acceleration: BrowserAcceleration },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputProcessing {
    pub confidence_threshold: f32,
    pub suppress_overlaps_iou: Option<f32>,
}

impl OutputProcessing {
    pub fn iou_threshold(&self) -> f32 {
        self.suppress_overlaps_iou.unwrap_or(0.5)
    }

    pub fn validate(&self) -> DomainResult<()> {
        if !self.confidence_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.confidence_threshold)
            || !self.iou_threshold().is_finite()
            || !(0.0..=1.0).contains(&self.iou_threshold())
        {
            return Err(DomainError::InvalidGeometry(
                "invalid prelabel processing thresholds".into(),
            ));
        }
        Ok(())
    }
}

/// Static float32 Ultralytics YOLO exports with batch=1, dynamic=false and nms=false.
/// Model class indices map explicitly to dataset IDs; None ignores an output class.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct YoloModelSpec {
    pub input_size: u32,
    /// Legacy positional mapping, retained when reading existing configurations.
    #[serde(default, with = "class_mapping", skip_serializing_if = "Vec::is_empty")]
    #[schemars(with = "Vec<String>")]
    pub class_ids: Vec<Option<ClassId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_mappings: Vec<YoloClassMapping>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_digest: Option<String>,
    #[serde(default)]
    pub keypoints: Vec<String>,
}

// TOML has no null array elements. A dash marks an ignored model output class.
mod class_mapping {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        ids: &[Option<ClassId>],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        ids.iter()
            .map(|id| id.as_ref().map_or("-", |id| id.as_str()))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Option<ClassId>>, D::Error> {
        Ok(Vec::<String>::deserialize(deserializer)?
            .into_iter()
            .map(|id| {
                if id == "-" {
                    None
                } else {
                    Some(ClassId::from(id))
                }
            })
            .collect())
    }
}

impl PrelabelConfig {
    pub fn validate(&self) -> DomainResult<()> {
        let invalid =
            || DomainError::InvalidGeometry("invalid prelabel model configuration".into());
        self.config_id
            .validate_path_segment()
            .map_err(|_| invalid())?;
        self.output_processing.validate()?;
        ModelSpec::validate_location(&self.model.location)?;
        if self.name.trim().is_empty() || self.model.model_id.trim().is_empty() {
            return Err(invalid());
        }
        if let PrelabelExecution::ServerSide { command } = &self.execution
            && !command.is_empty()
        {
            return Err(DomainError::InvalidGeometry(
                "server prelabels use the managed ONNX runner, not commands".into(),
            ));
        }
        let spec = self.yolo.as_ref().ok_or_else(invalid)?;
        if !(32..=1280).contains(&spec.input_size)
            || spec.input_size % 32 != 0
            || !(1..=1000).contains(&spec.model_class_count())
            || spec.mapped_classes().next().is_none()
            || spec.keypoints.len() > 256
        {
            return Err(invalid());
        }
        spec.validate_mapping()?;
        for id in spec.mapped_classes() {
            id.validate_path_segment().map_err(|_| invalid())?;
        }
        let mut names = std::collections::BTreeSet::new();
        for name in &spec.keypoints {
            if name.trim().is_empty() || name.len() > 128 || !names.insert(name) {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub fn validate_for_task(&self, task: &TaskDefinition) -> DomainResult<()> {
        self.validate()?;
        let spec = self.yolo.as_ref().expect("validated model profile");
        let compatible = task.enabled
            && task.prelabel_config_ids.contains(&self.config_id)
            && task
                .class_ids
                .iter()
                .all(|id| spec.mapped_classes().any(|mapped| mapped == id))
            && match task.annotation_type {
                crate::AnnotationType::BoundingBox => spec.keypoints.is_empty(),
                crate::AnnotationType::Skeleton => task.skeleton.as_ref().is_some_and(|s| {
                    !spec.keypoints.is_empty()
                        && s.keypoints
                            .iter()
                            .map(|k| &k.name)
                            .eq(spec.keypoints.iter())
                }),
            };
        if !compatible {
            return Err(DomainError::InvalidGeometry(
                "prelabel model is incompatible with workflow".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrelabelExecutionKind {
    ServerCpu,
    ServerCuda,
    ServerWebGpu,
    BrowserWebGpu,
    BrowserCpu,
}

impl PrelabelExecutionKind {
    pub fn is_server(&self) -> bool {
        matches!(
            self,
            Self::ServerCpu | Self::ServerCuda | Self::ServerWebGpu
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelInferenceResult {
    pub execution: PrelabelExecutionKind,
    pub suggestions: Vec<PrelabelSuggestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PredictionTrust {
    ServerGenerated,
    BrowserReported,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelProvenance {
    pub dataset_id: DatasetId,
    pub image_id: ImageId,
    pub image_hash: String,
    pub task_id: TaskId,
    pub class_id: ClassId,
    pub config_id: PrelabelConfigId,
    pub config_digest: String,
    pub model_id: String,
    pub model_version: Option<String>,
    pub model_digest: String,
    pub execution: PrelabelExecutionKind,
    pub trust: PredictionTrust,
    pub processing: OutputProcessing,
    pub generation: u64,
    pub scope_generation: u64,
    pub suggestion_id: String,
    pub confidence: f32,
}

/// The signature authorizes this exact suggestion, never an annotation mutation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelEvidence {
    pub provenance: PrelabelProvenance,
    pub predicted_geometry: AnnotationGeometry,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedPrelabel {
    pub provenance: PrelabelProvenance,
    pub predicted_geometry: AnnotationGeometry,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelSuggestion {
    pub suggestion_id: String,
    pub config_id: PrelabelConfigId,
    pub task_id: TaskId,
    pub class_id: ClassId,
    pub confidence: f32,
    pub geometry: AnnotationGeometry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Box<PrelabelEvidence>>,
}

impl PrelabelSuggestion {
    pub fn passes(&self, processing: &OutputProcessing) -> bool {
        self.confidence >= processing.confidence_threshold
    }
}
