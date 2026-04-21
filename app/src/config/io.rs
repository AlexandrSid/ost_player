use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
use crate::persist;
use std::fs;
use std::path::Path;

use super::AppConfig;

const CONFIG_HELP_MARKER_LINE: &str = "# ost_player config";
const CONFIG_HELP_HEADER: &str = r#"# ost_player config
#
# Audio
# - audio.volume_default_percent: initial volume for the current app session (0..=100)
#   - legacy alias accepted on load: audio.default_volume_percent
# - audio.volume_available_percent: discrete volume ladder (0..=100), must be unique + sorted and include 0 and 100
#   - legacy migration accepted on load: audio.volume_step_percent (generates [0, step, 2*step, ... 100] if the ladder is missing)
#
# Logging
# - logging.default_level: default | debug | trace
#   - default: includes ERROR/FATAL + config-changing domain events; excludes playback chatter
#   - debug/trace: more app logs (dependencies are still suppressed unless overridden)
# - logging.retention_days: delete log files older than this many days on startup
# - Rotation: logs are written into ~10-day buckets (3 files/month) under data/logs/
#
# Precedence:
# - If `RUST_LOG` is set, it overrides logging.default_level and dependency suppression rules.
#   This is useful for one-off debugging without editing config.yaml.
#
"#;

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
    let yaml = serde_yaml::to_string(cfg).map_err(|e| AppError::Config {
        message: format!("failed to serialize config: {e}"),
    })?;

    let preamble = if path.exists() {
        let existing = fs::read_to_string(path).unwrap_or_default();
        let (preserved, _rest) = split_leading_comment_preamble(&existing);
        if preserved.contains(CONFIG_HELP_MARKER_LINE) {
            preserved
        } else {
            format!("{CONFIG_HELP_HEADER}{preserved}")
        }
    } else {
        CONFIG_HELP_HEADER.to_string()
    };

    // Exactly one blank line between preamble and YAML.
    let serialized = format!("{}\n\n{}", preamble.trim_end(), yaml.trim_start());
    persist::write_text_safely(path, &serialized)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn split_leading_comment_preamble(s: &str) -> (String, String) {
    let mut preamble = String::new();
    let mut rest_start = 0usize;
    for (idx, line) in s.lines().enumerate() {
        let keep = line.trim().is_empty() || line.trim_start().starts_with('#');
        if keep {
            preamble.push_str(line);
            preamble.push('\n');
            rest_start = idx + 1;
        } else {
            break;
        }
    }
    let rest = s.lines().skip(rest_start).collect::<Vec<_>>().join("\n");
    (preamble, rest)
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
        assert_eq!(
            cfg.folders[0].scan_depth,
            crate::config::ScanDepth::RootOnly
        );
        assert_eq!(cfg.folders[1].path, "D:\\More");
        assert_eq!(
            cfg.folders[1].scan_depth,
            crate::config::ScanDepth::RootOnly
        );
    }

    #[test]
    fn split_leading_comment_preamble_preserves_only_leading_comment_and_blank_lines() {
        let s = "# a\n# b\n\nschema_version: 1\nsettings:\n  supported_extensions: [mp3]\n";
        let (preamble, rest) = split_leading_comment_preamble(s);
        assert!(preamble.starts_with("# a\n# b\n\n"));
        assert!(rest.starts_with("schema_version: 1"));
    }
}
