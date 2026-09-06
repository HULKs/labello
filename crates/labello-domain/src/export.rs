//! Pure selection and geometry policy for versioned ground-truth exports.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AnnotationType, BoundingBox, ClassId, DatasetMetadata, ImageDimensions, ImageId,
    SkeletonGeometry, SkeletonSpec, TaskId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportProfile {
    UltralyticsYoloDetectV1,
    UltralyticsYoloPoseV1,
}

impl ExportProfile {
    pub fn annotation_type(self) -> AnnotationType {
        match self {
            Self::UltralyticsYoloDetectV1 => AnnotationType::BoundingBox,
            Self::UltralyticsYoloPoseV1 => AnnotationType::Skeleton,
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ExportSplit {
    Train,
    Val,
    Test,
}

impl ExportSplit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Train => "train",
            Self::Val => "val",
            Self::Test => "test",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportClassSelection {
    pub task_id: TaskId,
    pub class_id: ClassId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportOptions {
    pub profile: ExportProfile,
    pub classes: BTreeSet<ExportClassSelection>,
    /// Required even when every current image already has split provenance.
    pub fallback_split: ExportSplit,
    #[serde(default)]
    pub split_choices: BTreeMap<ImageId, ExportSplit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportClassMapping {
    pub index: u32,
    pub selection: ExportClassSelection,
    pub name: String,
    pub skeleton: Option<SkeletonSpec>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum ExportPolicyError {
    #[error("choose between one and 256 task/class mappings")]
    SelectionSize,
    #[error("a selected task or class is missing or incompatible with the profile")]
    IncompatibleSelection,
    #[error("one class cannot be exported through multiple task identities")]
    AmbiguousClass,
    #[error("a pose has more than one current linked box")]
    AmbiguousBox,
    #[error("selected pose classes must have equal nonempty keypoint counts")]
    IncompatibleSkeletons,
    #[error("conflicting split provenance requires an explicit split choice")]
    SplitConflict,
    #[error("the image or annotation has invalid geometry")]
    InvalidGeometry,
    #[error("a pose requires a valid linked box or at least one placed keypoint")]
    PoseWithoutBounds,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ExportOmissionReason {
    Unfinished,
    ExcludedCoverage,
    IncompleteCoverage,
    UnverifiedAnnotations,
    ChangedReviewPolicy,
    UnresolvedMigration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportPoseBox {
    pub bounds: BoundingBox,
    pub derived: bool,
    pub linked_annotation: Option<(crate::AnnotationId, u32)>,
}

impl ExportOptions {
    /// Stable task/class ordering preserves distinct identities even when names match.
    pub fn class_mapping(
        &self,
        dataset: &DatasetMetadata,
    ) -> Result<Vec<ExportClassMapping>, ExportPolicyError> {
        if self.classes.is_empty() || self.classes.len() > 256 {
            return Err(ExportPolicyError::SelectionSize);
        }
        let mut class_ids = BTreeSet::new();
        let mut keypoint_count = None;
        self.classes
            .iter()
            .enumerate()
            .map(|(index, selection)| {
                let task = dataset
                    .task(&selection.task_id)
                    .filter(|task| {
                        task.annotation_type == self.profile.annotation_type()
                            && task.allows_class(&selection.class_id)
                    })
                    .ok_or(ExportPolicyError::IncompatibleSelection)?;
                let class = dataset
                    .label_classes
                    .iter()
                    .find(|class| class.class_id == selection.class_id)
                    .ok_or(ExportPolicyError::IncompatibleSelection)?;
                if !class_ids.insert(&selection.class_id) {
                    return Err(ExportPolicyError::AmbiguousClass);
                }
                let skeleton = if self.profile == ExportProfile::UltralyticsYoloPoseV1 {
                    let spec = task
                        .skeleton
                        .as_ref()
                        .ok_or(ExportPolicyError::IncompatibleSkeletons)?;
                    let names = spec
                        .keypoints
                        .iter()
                        .map(|point| &point.name)
                        .collect::<BTreeSet<_>>();
                    if names.is_empty()
                        || names.len() != spec.keypoints.len()
                        || names.iter().any(|name| name.trim().is_empty())
                        || keypoint_count.is_some_and(|count| count != spec.keypoints.len())
                    {
                        return Err(ExportPolicyError::IncompatibleSkeletons);
                    }
                    keypoint_count = Some(spec.keypoints.len());
                    Some(spec.clone())
                } else {
                    None
                };
                Ok(ExportClassMapping {
                    index: index as u32,
                    selection: selection.clone(),
                    name: class.name.clone(),
                    skeleton,
                })
            })
            .collect()
    }

    pub fn image_split(
        &self,
        image_id: &ImageId,
        memberships: &[String],
    ) -> Result<ExportSplit, ExportPolicyError> {
        if let Some(choice) = self.split_choices.get(image_id) {
            return Ok(*choice);
        }
        let splits = memberships
            .iter()
            .filter_map(|membership| match membership.as_str() {
                "train" => Some(ExportSplit::Train),
                "val" => Some(ExportSplit::Val),
                "test" => Some(ExportSplit::Test),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        match splits.len() {
            0 => Ok(self.fallback_split),
            1 => Ok(*splits.first().expect("one split")),
            _ => Err(ExportPolicyError::SplitConflict),
        }
    }
}

/// An explicitly linked box is supplied only after the caller checks its current identity.
/// Point-derived bounds use original-image dimensions, including points on an image edge.
pub fn export_pose_bounds(
    skeleton: &SkeletonGeometry,
    dimensions: ImageDimensions,
    linked_box: Option<BoundingBox>,
) -> Result<(BoundingBox, bool), ExportPolicyError> {
    dimensions
        .validate()
        .map_err(|_| ExportPolicyError::InvalidGeometry)?;
    skeleton
        .validate()
        .map_err(|_| ExportPolicyError::InvalidGeometry)?;
    if let Some(bbox) = linked_box {
        bbox.validate()
            .map_err(|_| ExportPolicyError::InvalidGeometry)?;
        return Ok((bbox, false));
    }
    let mut points = skeleton
        .keypoints
        .iter()
        .filter_map(|keypoint| keypoint.point);
    let first = points.next().ok_or(ExportPolicyError::PoseWithoutBounds)?;
    let (mut left, mut top, mut right, mut bottom) = (first.x, first.y, first.x, first.y);
    for point in points {
        left = left.min(point.x);
        top = top.min(point.y);
        right = right.max(point.x);
        bottom = bottom.max(point.y);
    }
    fn axis(low: f32, high: f32, pixels: u32) -> (f32, f32) {
        let width = (f64::from(high) - f64::from(low))
            .max(1.0 / f64::from(pixels))
            .min(1.0);
        let start = ((f64::from(low) + f64::from(high) - width) / 2.0).clamp(0.0, 1.0 - width);
        (start as f32, width as f32)
    }
    let (x, width) = axis(left, right, dimensions.width);
    let (y, height) = axis(top, bottom, dimensions.height);
    let bbox = BoundingBox {
        x,
        y,
        width,
        height,
    };
    bbox.validate()
        .map_err(|_| ExportPolicyError::InvalidGeometry)?;
    Ok((bbox, true))
}

#[cfg(test)]
mod tests;
