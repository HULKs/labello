use serde::{Deserialize, Serialize};

/// Public artifact identity. Missing fields describe a development or incomplete build.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildIdentity {
    pub release_tag: Option<String>,
    pub source_commit: Option<String>,
}

impl BuildIdentity {
    pub fn from_metadata(release_tag: Option<&str>, source_commit: Option<&str>) -> Self {
        Self {
            release_tag: release_tag
                .filter(|value| valid_tag(value))
                .map(str::to_owned),
            source_commit: source_commit
                .filter(|value| valid_commit(value))
                .map(str::to_owned),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.release_tag.as_deref().is_none_or(valid_tag)
            && self.source_commit.as_deref().is_none_or(valid_commit)
    }

    pub fn differs_from(&self, other: &Self) -> bool {
        self.is_valid()
            && other.is_valid()
            && self.release_tag.is_some()
            && self.source_commit.is_some()
            && other.release_tag.is_some()
            && other.source_commit.is_some()
            && self != other
    }
}

fn valid_tag(value: &str) -> bool {
    !value.is_empty()
        && value != "development"
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
}

fn valid_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub trait BuildInformationApi {
    fn build_information(&self) -> crate::ApiFuture<'_, BuildIdentity>;
}

impl BuildInformationApi for crate::HttpLabelloApi {
    fn build_information(&self) -> crate::ApiFuture<'_, BuildIdentity> {
        self.get_build_information()
    }
}

impl BuildInformationApi for crate::DemoLabelloApi {
    fn build_information(&self) -> crate::ApiFuture<'_, BuildIdentity> {
        Box::pin(async { Ok(BuildIdentity::default()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_requires_complete_bounded_artifact_metadata() {
        let a = BuildIdentity::from_metadata(Some("v1.2.3"), Some(&"a".repeat(40)));
        let b = BuildIdentity::from_metadata(Some("v1.2.4"), Some(&"b".repeat(40)));
        assert!(!a.differs_from(&a));
        assert!(a.differs_from(&b));
        assert!(a.differs_from(&BuildIdentity {
            release_tag: a.release_tag.clone(),
            ..b.clone()
        }));
        assert!(a.differs_from(&BuildIdentity {
            source_commit: a.source_commit.clone(),
            ..b
        }));
        for incomplete in [
            BuildIdentity::default(),
            BuildIdentity::from_metadata(Some("v1.2.3"), None),
            BuildIdentity::from_metadata(None, Some(&"a".repeat(40))),
        ] {
            assert!(!a.differs_from(&incomplete));
        }
        assert_eq!(
            BuildIdentity::from_metadata(Some("development"), Some("development")),
            BuildIdentity::default()
        );
        assert_eq!(
            BuildIdentity::from_metadata(Some(&"x".repeat(65)), Some("not-a-commit")),
            BuildIdentity::default()
        );
        assert!(
            !BuildIdentity {
                release_tag: Some("bad\nvalue".into()),
                source_commit: a.source_commit.clone()
            }
            .is_valid()
        );
    }
}
