use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    ClassId, DatasetId, DatasetRoleAssignment, ImageDimensions, ImageId, ImportId, LabelClass,
    MigrationRecord, PrelabelConfig, SCHEMA_VERSION, TaskDefinition, TaskId, TaskStatus, Timestamp,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetMetadata {
    pub schema_version: u32,
    pub dataset_id: DatasetId,
    pub name: String,
    pub dataset_root: String,
    pub image_roots: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub migration_history: Vec<MigrationRecord>,
    pub label_classes: Vec<LabelClass>,
    pub tasks: Vec<TaskDefinition>,
    pub images: BTreeMap<ImageId, ImageRecord>,
    pub role_assignments: Vec<DatasetRoleAssignment>,
    pub imbalance: Option<ImbalanceConfig>,
    pub prelabel_configs: Vec<PrelabelConfig>,
}

impl DatasetMetadata {
    pub fn new(dataset_id: DatasetId, name: impl Into<String>, timestamp: Timestamp) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            dataset_id,
            name: name.into(),
            dataset_root: ".".to_string(),
            image_roots: vec!["images".to_string()],
            created_at: timestamp,
            updated_at: timestamp,
            migration_history: Vec::new(),
            label_classes: Vec::new(),
            tasks: Vec::new(),
            images: BTreeMap::new(),
            role_assignments: Vec::new(),
            imbalance: None,
            prelabel_configs: Vec::new(),
        }
    }

    pub fn task(&self, task_id: &crate::TaskId) -> Option<&TaskDefinition> {
        self.tasks.iter().find(|task| &task.task_id == task_id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetConfig {
    pub schema_version: u32,
    pub dataset_id: DatasetId,
    pub name: String,
    pub dataset_root: String,
    pub image_roots: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub migration_history: Vec<MigrationRecord>,
    pub label_classes: Vec<LabelClass>,
    pub tasks: Vec<TaskDefinition>,
    pub role_assignments: Vec<DatasetRoleAssignment>,
    pub imbalance: Option<ImbalanceConfig>,
    pub prelabel_configs: Vec<PrelabelConfig>,
}

impl DatasetConfig {
    pub fn from_metadata(metadata: &DatasetMetadata) -> Self {
        Self {
            schema_version: metadata.schema_version,
            dataset_id: metadata.dataset_id.clone(),
            name: metadata.name.clone(),
            dataset_root: metadata.dataset_root.clone(),
            image_roots: if metadata.image_roots.is_empty() {
                vec!["images".to_string()]
            } else {
                metadata.image_roots.clone()
            },
            created_at: metadata.created_at,
            updated_at: metadata.updated_at,
            migration_history: metadata.migration_history.clone(),
            label_classes: metadata.label_classes.clone(),
            tasks: metadata.tasks.clone(),
            role_assignments: metadata.role_assignments.clone(),
            imbalance: metadata.imbalance.clone(),
            prelabel_configs: metadata.prelabel_configs.clone(),
        }
    }

    pub fn into_metadata(self, images: BTreeMap<ImageId, ImageRecord>) -> DatasetMetadata {
        DatasetMetadata {
            schema_version: self.schema_version,
            dataset_id: self.dataset_id,
            name: self.name,
            dataset_root: self.dataset_root,
            image_roots: if self.image_roots.is_empty() {
                vec!["images".to_string()]
            } else {
                self.image_roots
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
            migration_history: self.migration_history,
            label_classes: self.label_classes,
            tasks: self.tasks,
            images,
            role_assignments: self.role_assignments,
            imbalance: self.imbalance,
            prelabel_configs: self.prelabel_configs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageRecord {
    pub image_id: ImageId,
    pub blake3: String,
    pub canonical_path: String,
    pub known_paths: Vec<String>,
    pub duplicate_paths: Vec<String>,
    pub file_name: String,
    pub byte_size: u64,
    pub width: u32,
    pub height: u32,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_memberships: Option<Vec<String>>,
}

impl ImageRecord {
    pub fn dimensions(&self) -> ImageDimensions {
        ImageDimensions {
            width: self.width,
            height: self.height,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImagesIndex {
    pub schema_version: u32,
    #[serde(default)]
    pub image_count: usize,
    pub images_by_hash: BTreeMap<String, ImageRecord>,
}

impl Default for ImagesIndex {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            image_count: 0,
            images_by_hash: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
#[schemars(rename_all = "camelCase")]
pub struct ImbalanceConfig {
    pub policy: ImbalancePolicy,
    pub enforce: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[schemars(rename_all = "camelCase")]
pub enum ImbalancePolicy {
    Ratio { max_ratio: f32 },
    AbsoluteWindow { max_difference: u64 },
}

impl ImbalancePolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Ratio { max_ratio } if max_ratio.is_finite() && *max_ratio >= 1.0 => Ok(()),
            Self::Ratio { .. } => Err("imbalance ratio maxRatio must be finite and at least 1.0"),
            Self::AbsoluteWindow { .. } => Ok(()),
        }
    }

    pub fn blocks_count(&self, selected_count: usize, minimum_peer_count: usize) -> bool {
        match self {
            Self::Ratio { max_ratio } => {
                if minimum_peer_count == 0 {
                    selected_count > 0
                } else {
                    (selected_count as f64 / minimum_peer_count as f64) > f64::from(*max_ratio)
                }
            }
            Self::AbsoluteWindow { max_difference } => {
                selected_count.saturating_sub(minimum_peer_count) as u128
                    > u128::from(*max_difference)
            }
        }
    }

    pub fn blocked_tasks(
        &self,
        enabled_task_ids: &[TaskId],
        counts: &BTreeMap<TaskId, usize>,
    ) -> BTreeSet<TaskId> {
        if enabled_task_ids.len() < 2 {
            return BTreeSet::new();
        }
        enabled_task_ids
            .iter()
            .filter_map(|selected_task_id| {
                let selected_count = counts.get(selected_task_id).copied().unwrap_or_default();
                let minimum_peer_count = enabled_task_ids
                    .iter()
                    .filter(|task_id| *task_id != selected_task_id)
                    .map(|task_id| counts.get(task_id).copied().unwrap_or_default())
                    .min()
                    .expect("at least one enabled peer must exist");
                self.blocks_count(selected_count, minimum_peer_count)
                    .then(|| selected_task_id.clone())
            })
            .collect()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImbalanceConfigRef<'a> {
    policy: &'a ImbalancePolicy,
    enforce: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ImbalanceConfigWire {
    Current(CurrentImbalanceConfig),
    LegacyRatio(LegacyRatioImbalanceConfig),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CurrentImbalanceConfig {
    policy: ImbalancePolicy,
    enforce: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRatioImbalanceConfig {
    max_ratio: f32,
    enforce: bool,
}

impl Serialize for ImbalanceConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ImbalanceConfigRef {
            policy: &self.policy,
            enforce: self.enforce,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ImbalanceConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match ImbalanceConfigWire::deserialize(deserializer)? {
            ImbalanceConfigWire::Current(config) => Self {
                policy: config.policy,
                enforce: config.enforce,
            },
            ImbalanceConfigWire::LegacyRatio(config) => Self {
                policy: ImbalancePolicy::Ratio {
                    max_ratio: config.max_ratio,
                },
                enforce: config.enforce,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFileEntry {
    pub path: String,
    pub byte_size: u64,
    pub blake3: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatasetSnapshot {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub dataset_id: DatasetId,
    pub created_at: Timestamp,
    pub includes_image_bytes: bool,
    pub total_bytes: u64,
    pub files: Vec<SnapshotFileEntry>,
    #[serde(default)]
    pub imports: Vec<SnapshotImportEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotImportEntry {
    pub import_id: ImportId,
    pub manifest_path: String,
    pub source_objects_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageExplorerItem {
    pub image: ImageRecord,
    pub task_statuses: BTreeMap<TaskId, TaskStatus>,
    pub class_ids: BTreeSet<ClassId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageExplorerPage {
    pub items: Vec<ImageExplorerItem>,
    pub page: usize,
    pub page_size: usize,
    pub total_items: usize,
    pub total_pages: usize,
}

impl Default for ImbalanceConfig {
    fn default() -> Self {
        Self {
            policy: ImbalancePolicy::Ratio { max_ratio: 2.0 },
            enforce: false,
        }
    }
}

#[cfg(test)]
mod imbalance_tests {
    use super::*;

    #[test]
    fn absolute_window_uses_strict_gap_boundary_and_zero_counts() {
        let policy = ImbalancePolicy::AbsoluteWindow { max_difference: 2 };

        assert!(!policy.blocks_count(0, 0));
        assert!(!policy.blocks_count(2, 0));
        assert!(policy.blocks_count(3, 0));
        assert!(!policy.blocks_count(7, 5));
        assert!(policy.blocks_count(8, 5));
        assert!(!policy.blocks_count(3, 8));
    }

    #[test]
    fn blocked_tasks_considers_only_the_supplied_enabled_peers() {
        let first = TaskId::from("first");
        let second = TaskId::from("second");
        let disabled = TaskId::from("disabled");
        let policy = ImbalancePolicy::AbsoluteWindow { max_difference: 1 };
        let counts = BTreeMap::from([
            (first.clone(), 2),
            (second.clone(), 0),
            (disabled.clone(), 100),
        ]);

        assert_eq!(
            policy.blocked_tasks(&[first.clone(), second], &counts),
            BTreeSet::from([first.clone()])
        );
        assert!(policy.blocked_tasks(&[first], &counts).is_empty());
        assert!(policy.blocked_tasks(&[], &counts).is_empty());
    }

    #[test]
    fn legacy_ratio_configuration_deserializes_into_tagged_policy() {
        let legacy: ImbalanceConfig = serde_json::from_value(serde_json::json!({
            "maxRatio": 2.0,
            "enforce": true
        }))
        .unwrap();
        assert_eq!(
            legacy,
            ImbalanceConfig {
                policy: ImbalancePolicy::Ratio { max_ratio: 2.0 },
                enforce: true,
            }
        );

        assert_eq!(
            serde_json::to_value(legacy).unwrap(),
            serde_json::json!({
                "policy": { "kind": "ratio", "maxRatio": 2.0 },
                "enforce": true
            })
        );
    }

    #[test]
    fn policy_validation_rejects_invalid_ratios_and_accepts_any_absolute_window() {
        for max_ratio in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 0.999] {
            assert!(ImbalancePolicy::Ratio { max_ratio }.validate().is_err());
        }
        for max_ratio in [1.0, 2.0, f32::MAX] {
            assert!(ImbalancePolicy::Ratio { max_ratio }.validate().is_ok());
        }
        assert!(
            ImbalancePolicy::AbsoluteWindow { max_difference: 0 }
                .validate()
                .is_ok()
        );
        assert!(
            ImbalancePolicy::AbsoluteWindow {
                max_difference: u64::MAX,
            }
            .validate()
            .is_ok()
        );
    }
}
