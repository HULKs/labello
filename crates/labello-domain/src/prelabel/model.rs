use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct YoloClassMapping {
    pub model_class_id: u32,
    pub class_id: ClassId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelTensor {
    pub name: String,
    pub data_type: String,
    pub shape: Vec<Option<i64>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct YoloOutputProfile {
    pub class_count: u32,
    /// Empty when class names are unavailable; numeric output IDs remain authoritative.
    pub class_names: Vec<String>,
    pub keypoint_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelModelOutput {
    pub tensor: PrelabelTensor,
    pub profile: Option<YoloOutputProfile>,
    pub problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrelabelModelInspection {
    pub model_digest: String,
    pub inputs: Vec<PrelabelTensor>,
    pub input_size: Option<u32>,
    pub outputs: Vec<PrelabelModelOutput>,
    pub problem: Option<String>,
}

impl ModelSpec {
    pub fn validate_location(location: &str) -> DomainResult<()> {
        if location.is_empty()
            || location.len() > 128
            || !location.ends_with(".onnx")
            || !location
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || location.starts_with('.')
        {
            return Err(DomainError::InvalidGeometry(
                "use an ONNX filename in the configured models directory".into(),
            ));
        }
        Ok(())
    }
}

impl YoloModelSpec {
    pub fn model_class_count(&self) -> usize {
        self.class_count
            .map_or(self.class_ids.len(), |count| count as usize)
    }

    pub fn dataset_class(&self, model_class_id: usize) -> Option<&ClassId> {
        if self.class_count.is_some() {
            self.class_mappings
                .iter()
                .find(|mapping| mapping.model_class_id as usize == model_class_id)
                .map(|mapping| &mapping.class_id)
        } else {
            self.class_ids.get(model_class_id).and_then(Option::as_ref)
        }
    }

    pub fn mapped_classes(&self) -> impl Iterator<Item = &ClassId> {
        self.class_ids
            .iter()
            .flatten()
            .chain(self.class_mappings.iter().map(|m| &m.class_id))
    }

    pub(super) fn validate_mapping(&self) -> DomainResult<()> {
        let invalid =
            || DomainError::InvalidGeometry("invalid model output or class mapping".into());
        if self
            .output_name
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.len() > 256)
            || self
                .model_digest
                .as_ref()
                .is_some_and(|s| s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(invalid());
        }
        if let Some(count) = self.class_count {
            if !self.class_ids.is_empty()
                || self.output_name.is_none()
                || self.class_mappings.len() > 1000
            {
                return Err(invalid());
            }
            let mut outputs = std::collections::BTreeSet::new();
            for mapping in &self.class_mappings {
                if mapping.model_class_id >= count || !outputs.insert(mapping.model_class_id) {
                    return Err(invalid());
                }
            }
        } else if !self.class_mappings.is_empty() {
            return Err(invalid());
        }
        Ok(())
    }

    /// Make the old positional representation explicit without changing its meaning.
    pub fn use_explicit_mapping(&mut self, count: u32) {
        if self.class_count.is_none() {
            self.class_mappings = self
                .class_ids
                .iter()
                .enumerate()
                .filter_map(|(id, class)| {
                    class.as_ref().map(|class_id| YoloClassMapping {
                        model_class_id: id as u32,
                        class_id: class_id.clone(),
                    })
                })
                .collect();
            self.class_ids.clear();
        }
        self.class_count = Some(count);
        // Keep out-of-range mappings for validation; never silently change their meaning.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_mapping_round_trips_and_converts_without_reassigning_classes() {
        let wire =
            serde_json::json!({"inputSize":320,"classIds":["person","-","ball"],"keypoints":[]});
        let mut spec: YoloModelSpec = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&spec).unwrap(), wire);
        spec.use_explicit_mapping(80);
        spec.output_name = Some("output0".into());
        spec.validate_mapping().unwrap();
        assert_eq!(spec.model_class_count(), 80);
        assert_eq!(spec.dataset_class(0).unwrap().as_str(), "person");
        assert_eq!(spec.dataset_class(1), None);
        assert_eq!(spec.dataset_class(2).unwrap().as_str(), "ball");
        let explicit = serde_json::to_value(&spec).unwrap();
        assert!(explicit.get("classIds").is_none());
        assert_eq!(
            serde_json::from_value::<YoloModelSpec>(explicit).unwrap(),
            spec
        );
    }

    #[test]
    fn explicit_mapping_rejects_duplicate_out_of_range_and_mixed_legacy_entries() {
        let mut spec = YoloModelSpec {
            class_count: Some(80),
            output_name: Some("output0".into()),
            class_mappings: vec![YoloClassMapping {
                model_class_id: 32,
                class_id: "ball".into(),
            }],
            ..Default::default()
        };
        assert!(spec.validate_mapping().is_ok());
        spec.class_mappings.push(spec.class_mappings[0].clone());
        assert!(spec.validate_mapping().is_err());
        spec.class_mappings.pop();
        spec.class_mappings[0].model_class_id = 80;
        assert!(spec.validate_mapping().is_err());
        spec.class_mappings[0].model_class_id = 0;
        spec.class_ids.push(Some("person".into()));
        assert!(spec.validate_mapping().is_err());
        spec.class_ids.clear();
        spec.output_name = None;
        assert!(spec.validate_mapping().is_err());
    }
}
