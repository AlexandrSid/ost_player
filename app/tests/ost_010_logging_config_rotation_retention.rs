use ost_player::config::io as config_io;
use ost_player::config::{AppConfig, LoggingLevel};
use ost_player::paths::AppPaths;
use serde_yaml::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn make_paths_in(base_dir: PathBuf) -> AppPaths {
    let data_dir = base_dir.join("data");
    let cache_dir = data_dir.join("cache");
    let logs_dir = data_dir.join("logs");
    let config_path = data_dir.join("config.yaml");
    let playlists_path = data_dir.join("playlists.yaml");
    let state_path = data_dir.join("state.yaml");
    AppPaths {
        base_dir,
        data_dir,
        cache_dir,
        logs_dir,
        config_path,
        playlists_path,
        state_path,
    }
}

fn read_yaml_value(path: &std::path::Path) -> Value {
    let raw = fs::read_to_string(path).expect("read yaml file");
    serde_yaml::from_str(&raw).expect("yaml should parse")
}

#[test]
fn logging_section_defaults_apply_when_missing() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    assert!(matches!(cfg.logging.default_level, LoggingLevel::Default));
    assert_eq!(cfg.logging.retention_days, 31);
}

#[test]
fn logging_retention_days_can_be_overridden_in_yaml() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
logging:
  retention_days: 7
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    assert_eq!(cfg.logging.retention_days, 7);
}

#[test]
fn logging_unknown_fields_are_preserved_for_forward_compat() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
logging:
  default_level: debug
  retention_days: 9
  future_flag: true
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    assert!(matches!(cfg.logging.default_level, LoggingLevel::Debug));
    assert_eq!(cfg.logging.retention_days, 9);
    assert!(
        cfg.logging.extra.contains_key("future_flag"),
        "logging.extra should preserve unknown fields"
    );
}

#[test]
fn load_or_create_writes_logging_section_in_default_config_file() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    let cfg = config_io::load_or_create(&paths).expect("should create default config");
    assert_eq!(cfg.logging.retention_days, 31);
    assert!(matches!(cfg.logging.default_level, LoggingLevel::Default));

    let v = read_yaml_value(&paths.config_path);
    assert!(
        v.get("logging").is_some(),
        "default config.yaml should include logging section"
    );
    assert_eq!(
        v.get("logging")
            .and_then(|l| l.get("retention_days"))
            .and_then(Value::as_i64),
        Some(31)
    );
}
