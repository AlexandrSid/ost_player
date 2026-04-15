use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
use crate::persist;
use std::fs;
use std::path::Path;

use super::AppConfig;

pub fn load_or_create(paths: &AppPaths) -> AppResult<AppConfig> {
    let path = &paths.config_path;
    if !path.exists() {
        persist::recover_missing_final(path)?;
    }

    if !path.exists() {
        let cfg = AppConfig::default();
        save(paths, &cfg)?;
        return Ok(cfg);
    }

    let raw = fs::read_to_string(path).map_err(|e| AppError::Io {
        path: path.clone(),
        source: e,
    })?;

    let cfg: AppConfig = serde_yaml::from_str(&raw).map_err(|e| AppError::Config {
        message: format!("failed to parse `{}`: {e}", display_path(path)),
    })?;

    cfg.validate().map_err(|msg| AppError::Config {
        message: format!("invalid `{}`: {msg}", display_path(path)),
    })?;

    Ok(cfg.normalized())
}

pub fn save(paths: &AppPaths, cfg: &AppConfig) -> AppResult<()> {
    let path = &paths.config_path;
    let serialized = serde_yaml::to_string(cfg).map_err(|e| AppError::Config {
        message: format!("failed to serialize config: {e}"),
    })?;

    persist::write_text_safely(path, &serialized)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use std::path::PathBuf;

    fn paths_for(dir: &std::path::Path) -> AppPaths {
        let base_dir = dir.to_path_buf();
        let data_dir = base_dir.join("data");
        AppPaths {
            base_dir,
            cache_dir: data_dir.join("cache"),
            logs_dir: data_dir.join("logs"),
            playlists_dir: data_dir.join("playlists"),
            config_path: data_dir.join("config.yaml"),
            playlists_path: data_dir.join("playlists.yaml"),
            state_path: data_dir.join("state.yaml"),
            data_dir,
        }
    }

    #[test]
    fn load_or_create_restores_from_bak_when_final_missing() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        std::fs::create_dir_all(paths.data_dir.clone()).unwrap();
        let bak = PathBuf::from(format!("{}.bak", paths.config_path.to_string_lossy()));
        std::fs::write(
            &bak,
            r#"
schema_version: 1
settings:
  shuffle: true
  supported_extensions: [mp3]
"#,
        )
        .unwrap();

        let cfg = load_or_create(&paths).unwrap();
        assert!(cfg.settings.shuffle);
        assert!(paths.config_path.exists());
        assert!(!bak.exists());
    }

    #[test]
    fn load_or_create_restores_from_tmp_when_final_missing() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        std::fs::create_dir_all(paths.data_dir.clone()).unwrap();
        let tmp = PathBuf::from(format!("{}.tmp", paths.config_path.to_string_lossy()));
        std::fs::write(
            &tmp,
            r#"
schema_version: 1
settings:
  shuffle: true
  supported_extensions: [mp3]
"#,
        )
        .unwrap();

        let cfg = load_or_create(&paths).unwrap();
        assert!(cfg.settings.shuffle);
        assert!(paths.config_path.exists());
        assert!(!tmp.exists());
    }

    #[test]
    fn settings_allows_unknown_fields_for_forward_compat() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        std::fs::create_dir_all(paths.data_dir.clone()).unwrap();
        std::fs::write(
            &paths.config_path,
            r#"
schema_version: 1
settings:
  shuffle: true
  supported_extensions: [mp3]
  future_setting: 123
"#,
        )
        .unwrap();

        let cfg = load_or_create(&paths).unwrap();
        assert!(cfg.settings.shuffle);
        assert!(cfg.settings.extra.contains_key("future_setting"));
    }

    #[test]
    fn legacy_folders_vec_string_deserializes_as_root_only_true() {
        let raw = r#"
schema_version: 1
folders: ["C:\\Music", "D:\\More"]
settings:
  supported_extensions: [mp3]
"#;
        let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
        assert_eq!(cfg.folders.len(), 2);
        assert_eq!(cfg.folders[0].path, "C:\\Music");
        assert!(cfg.folders[0].root_only);
        assert_eq!(cfg.folders[1].path, "D:\\More");
        assert!(cfg.folders[1].root_only);
    }
}
