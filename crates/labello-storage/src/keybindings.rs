use labello_domain::{KeybindingSet, UserId};

use crate::{
    DatasetRepository, StorageResult,
    fstoml::{read_current_toml, write_toml_atomic},
    paths,
};

impl DatasetRepository {
    pub async fn existing_keybindings(
        &self,
        user: &UserId,
    ) -> StorageResult<Option<KeybindingSet>> {
        user.validate_path_segment()
            .map_err(|_| crate::StorageError::Unauthorized("invalid preference owner".into()))?;
        self.ensure_artifact_migration().await?;
        match read_current_toml::<KeybindingSet>(&self.keybindings_path(user)).await {
            Ok(mut value) => {
                value.normalize();
                value.validate()?;
                if &value.user_id != user {
                    return Err(crate::StorageError::Unauthorized(
                        "preference owner mismatch".into(),
                    ));
                }
                Ok(Some(value))
            }
            Err(crate::StorageError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn load_keybindings(&self, user_id: &UserId) -> StorageResult<KeybindingSet> {
        self.ensure_artifact_migration().await?;
        let path = self.keybindings_path(user_id);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|source| crate::StorageError::Io {
                path: path.clone(),
                source,
            })?
        {
            let mut keybindings: KeybindingSet = read_current_toml(&path).await?;
            keybindings.normalize();
            keybindings.validate()?;
            Ok(keybindings)
        } else {
            Ok(KeybindingSet::defaults_for(user_id.clone()))
        }
    }

    pub async fn save_keybindings(&self, keybindings: &KeybindingSet) -> StorageResult<()> {
        self.ensure_artifact_migration().await?;
        let mut keybindings = keybindings.clone();
        labello_domain::validate_schema_version(keybindings.schema_version)?;
        keybindings.validate()?;
        keybindings.normalize();
        keybindings.validate()?;
        write_toml_atomic(&self.keybindings_path(&keybindings.user_id), &keybindings).await
    }

    fn keybindings_path(&self, user_id: &UserId) -> std::path::PathBuf {
        self.root()
            .join(paths::USERS_DIR)
            .join(user_id.as_str())
            .join(paths::KEYBINDINGS_FILE)
    }
}

/// Account preferences shared by all datasets in one server process.
#[derive(Clone)]
pub struct UserPreferencesStore {
    root: std::path::PathBuf,
    lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}

impl UserPreferencesStore {
    pub fn new(datasets_root: impl AsRef<std::path::Path>) -> Self {
        Self {
            root: datasets_root.as_ref().join(".labello-server/users"),
            lock: Default::default(),
        }
    }

    fn path(&self, user: &UserId) -> StorageResult<std::path::PathBuf> {
        user.validate_path_segment()
            .map_err(|_| crate::StorageError::Unauthorized("invalid preference owner".into()))?;
        Ok(self.root.join(user.as_str()).join(paths::KEYBINDINGS_FILE))
    }

    pub async fn load(&self, user: &UserId) -> StorageResult<Option<KeybindingSet>> {
        let _guard = self.lock.lock().await;
        let path = self.path(user)?;
        match read_current_toml::<KeybindingSet>(&path).await {
            Ok(mut value) => {
                value.normalize();
                value.validate()?;
                if &value.user_id != user {
                    return Err(crate::StorageError::Unauthorized(
                        "preference owner mismatch".into(),
                    ));
                }
                Ok(Some(value))
            }
            Err(crate::StorageError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn save(&self, value: &KeybindingSet) -> StorageResult<()> {
        let _guard = self.lock.lock().await;
        let path = self.path(&value.user_id)?;
        labello_domain::validate_schema_version(value.schema_version)?;
        value.validate()?;
        let mut value = value.clone();
        value.normalize();
        value.validate()?;
        write_toml_atomic(&path, &value).await
    }
}

#[cfg(test)]
mod global_tests {
    use super::*;

    #[tokio::test]
    async fn global_preferences_survive_restart_and_invalid_save_preserves_previous_value() {
        let temp = tempfile::tempdir().unwrap();
        let store = UserPreferencesStore::new(temp.path());
        let user = UserId::from("person");
        assert!(store.load(&user).await.unwrap().is_none());
        let mut value = KeybindingSet::defaults_for(user.clone());
        value.pan_drag_modifier = labello_domain::PanDragModifier::Alt;
        value.bindings.insert(
            labello_domain::UserAction::DeleteAnnotation,
            labello_domain::KeyChord::new("MouseRight"),
        );
        store.save(&value).await.unwrap();
        let restarted = UserPreferencesStore::new(temp.path());
        assert_eq!(restarted.load(&user).await.unwrap(), Some(value.clone()));
        let mut invalid = value.clone();
        invalid.schema_version = 999;
        assert!(store.save(&invalid).await.is_err());
        assert_eq!(restarted.load(&user).await.unwrap(), Some(value));
        assert!(store.load(&UserId::from("../escape")).await.is_err());
        assert!(store.load(&UserId::from("other")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn global_preferences_ignore_interrupted_temporary_files_and_normalize_legacy_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = UserPreferencesStore::new(temp.path());
        let user = UserId::from("person");
        let mut legacy = KeybindingSet::defaults_for(user.clone());
        legacy.schema_version = 2;
        legacy
            .bindings
            .remove(&labello_domain::UserAction::RefocusObject);
        let path = store.path(&user).unwrap();
        write_toml_atomic(&path, &legacy).await.unwrap();
        tokio::fs::write(path.with_extension("tmp-interrupted"), "incomplete = [")
            .await
            .unwrap();
        let restarted = UserPreferencesStore::new(temp.path());
        let loaded = restarted.load(&user).await.unwrap().unwrap();
        assert_eq!(loaded.schema_version, labello_domain::SCHEMA_VERSION);
        assert!(
            loaded
                .bindings
                .contains_key(&labello_domain::UserAction::RefocusObject)
        );
        assert_eq!(
            crate::fstoml::read_toml::<KeybindingSet>(&path)
                .await
                .unwrap()
                .schema_version,
            2
        );
    }

    #[tokio::test]
    async fn concurrent_global_saves_publish_one_complete_value() {
        let temp = tempfile::tempdir().unwrap();
        let store = UserPreferencesStore::new(temp.path());
        let first = KeybindingSet::defaults_for(UserId::from("person"));
        let mut second = first.clone();
        second.pan_drag_modifier = labello_domain::PanDragModifier::Alt;
        let (one, two) = tokio::join!(store.save(&first), store.save(&second));
        one.unwrap();
        two.unwrap();
        let actual = store.load(&first.user_id).await.unwrap().unwrap();
        assert!(actual == first || actual == second);
    }
}

#[cfg(test)]
mod tests {
    use labello_domain::{KeyChord, UserAction};

    use super::*;

    #[tokio::test]
    async fn mouse_bindings_survive_repository_reload() {
        let temp = tempfile::tempdir().unwrap();
        let user = UserId::from("mouse_user");
        let mut bindings = KeybindingSet::defaults_for(user.clone());
        let mut chord = KeyChord::new("MouseRight");
        chord.shift = true;
        bindings
            .bindings
            .insert(UserAction::DeleteAnnotation, chord);
        DatasetRepository::new(temp.path())
            .save_keybindings(&bindings)
            .await
            .unwrap();
        let reloaded = DatasetRepository::new(temp.path())
            .load_keybindings(&user)
            .await
            .unwrap();
        assert_eq!(reloaded, bindings);
    }

    #[tokio::test]
    async fn load_normalizes_legacy_actions_and_save_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let repo = DatasetRepository::new(temp.path());
        let user_id = UserId::from("user_1");
        tokio::fs::create_dir_all(temp.path().join(paths::USERS_DIR).join(user_id.as_str()))
            .await
            .unwrap();
        let mut legacy = KeybindingSet::defaults_for(user_id.clone());
        legacy.bindings.clear();
        legacy
            .bindings
            .insert(UserAction::NextImage, KeyChord::new("X"));
        legacy
            .bindings
            .insert(UserAction::PreviousImage, KeyChord::new("ArrowLeft"));
        write_toml_atomic(&repo.keybindings_path(&user_id), &legacy)
            .await
            .unwrap();

        let loaded = repo.load_keybindings(&user_id).await.unwrap();
        assert_eq!(loaded.bindings[&UserAction::NextImage].key, "X");
        assert_ne!(
            loaded.bindings[&UserAction::NextImage],
            loaded.bindings[&UserAction::SkipAssignment]
        );
        assert_eq!(loaded.bindings[&UserAction::PreviousImage].key, "ArrowLeft");
        assert_eq!(loaded.bindings[&UserAction::RefocusObject].key, "R");
        assert_eq!(loaded.bindings.len(), UserAction::ACTIVE.len());
        assert_eq!(
            loaded.pan_drag_modifier,
            labello_domain::PanDragModifier::Control
        );

        repo.save_keybindings(&loaded).await.unwrap();
        assert_eq!(repo.load_keybindings(&user_id).await.unwrap(), loaded);
    }
}
