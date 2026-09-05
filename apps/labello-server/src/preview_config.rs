use anyhow::{Context, bail};
use labello_storage::{PreviewCache, PreviewConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreviewFileConfig {
    pub cache_root: String,
    pub limits: PreviewConfig,
}
impl Default for PreviewFileConfig {
    fn default() -> Self {
        Self {
            cache_root: ".labello-preview-cache".into(),
            limits: PreviewConfig::default(),
        }
    }
}

pub(crate) fn preview_cache(
    datasets_root: &Path,
    config: PreviewFileConfig,
) -> anyhow::Result<PreviewCache> {
    let datasets_root = resolved(datasets_root)?;
    let cache_root = resolved(Path::new(&config.cache_root))?;
    if cache_root.starts_with(&datasets_root) || datasets_root.starts_with(&cache_root) {
        bail!("previews.cacheRoot and datasetsRoot must be separate, non-overlapping directories");
    }
    PreviewCache::new(cache_root, config.limits).context("invalid preview configuration")
}

fn resolved(path: &Path) -> anyhow::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return resolved(Path::new("."));
    }
    if path.exists() {
        return path
            .canonicalize()
            .context("cannot resolve preview or dataset root");
    }
    let parent = path.parent().context("invalid preview or dataset root")?;
    Ok(resolved(parent)?.join(
        path.file_name()
            .context("invalid preview or dataset root")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_roots_cannot_overlap_dataset_authority() {
        for cache_root in [".", "./nested-preview-cache"] {
            assert!(
                preview_cache(
                    Path::new("."),
                    PreviewFileConfig {
                        cache_root: cache_root.into(),
                        ..Default::default()
                    }
                )
                .is_err()
            );
        }
        assert!(
            preview_cache(
                Path::new("./nested-datasets"),
                PreviewFileConfig {
                    cache_root: ".".into(),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn preview_limits_are_optional_strict_and_bounded() {
        let config: PreviewFileConfig = toml::from_str("[limits]\nworkers=1\n").unwrap();
        assert_eq!(config.limits.workers, 1);
        assert_eq!(
            config.limits.cache_bytes,
            PreviewConfig::default().cache_bytes
        );
        assert!(toml::from_str::<PreviewFileConfig>("unknown=1").is_err());
        assert!(toml::from_str::<PreviewFileConfig>("[limits]\nunknown=1").is_err());
        for workers in [0, 9] {
            assert!(
                PreviewConfig {
                    workers,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
    }
}
