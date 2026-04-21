use ost_player::config::io as config_io;
use ost_player::config::{AppConfig, FolderEntry, RepeatMode};
use ost_player::error::AppError;
use ost_player::paths::AppPaths;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn make_paths_in(base_dir: PathBuf) -> AppPaths {
    let data_dir = base_dir.join("data");
    let config_path = data_dir.join("config.yaml");
    let cache_dir = data_dir.join("cache");
    let logs_dir = data_dir.join("logs");
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

#[test]
fn paths_resolve_uses_current_exe_parent() {
    let exe = std::env::current_exe().expect("current_exe should work in tests");
    let expected_base = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let paths = AppPaths::resolve().expect("resolve should succeed");
    assert_eq!(paths.base_dir, expected_base);
    assert_eq!(paths.data_dir, expected_base.join("data"));
    assert_eq!(paths.cache_dir, expected_base.join("data").join("cache"));
    assert_eq!(paths.logs_dir, expected_base.join("data").join("logs"));
    assert_eq!(
        paths.config_path,
        expected_base.join("data").join("config.yaml")
    );
    assert_eq!(
        paths.playlists_path,
        expected_base.join("data").join("playlists.yaml")
    );
    assert_eq!(
        paths.state_path,
        expected_base.join("data").join("state.yaml")
    );
}

#[test]
fn ensure_data_dirs_creates_logs_dir() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    paths.ensure_data_dirs().expect("should create dirs");
    assert!(
        paths.logs_dir.is_dir(),
        "logs_dir should exist as directory"
    );
}

#[test]
fn ensure_data_dirs_does_not_create_playlists_subdir() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    paths.ensure_data_dirs().expect("should create dirs");
    assert!(
        !paths.data_dir.join("playlists").exists(),
        "data/playlists should not be created"
    );
}

#[test]
fn ensure_data_dirs_returns_io_error_if_logs_dir_is_a_file() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    fs::create_dir_all(&paths.data_dir).unwrap();
    fs::write(&paths.logs_dir, b"not a dir").unwrap();

    let err = paths.ensure_data_dirs().unwrap_err();
    match err {
        AppError::Io { path, .. } => {
            assert_eq!(path, paths.logs_dir);
        }
        other => panic!("expected AppError::Io, got {other:?}"),
    }
}

#[test]
fn load_or_create_creates_default_config_when_missing() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    // note: we intentionally do not create data_dir or config file; loader should create both.

    let cfg = config_io::load_or_create(&paths).expect("missing config should be created");
    assert!(paths.config_path.is_file(), "config should be created");
    assert_eq!(cfg.settings.min_size_kb, 1024);
    assert_eq!(cfg.settings.min_size_bytes, 1024 * 1024);
    assert!(!cfg.settings.shuffle);
    assert!(matches!(cfg.settings.repeat, RepeatMode::Off));
    assert!(cfg.folders.is_empty());
}

#[test]
fn load_or_create_parses_valid_yaml() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let yaml = r#"
settings:
  min_size_kb: 123
  shuffle: true
  repeat: all
  supported_extensions: ["mp3", "ogg"]
folders:
  - path: "C:\\Music"
  - path: "D:\\Other"
"#;
    fs::write(&paths.config_path, yaml).unwrap();

    let cfg = config_io::load_or_create(&paths).unwrap();
    assert_eq!(cfg.settings.min_size_kb, 123);
    assert_eq!(cfg.settings.min_size_bytes, 123 * 1024);
    assert!(cfg.settings.shuffle);
    assert!(matches!(cfg.settings.repeat, RepeatMode::All));
    assert_eq!(
        cfg.folders,
        vec![
            FolderEntry::new("C:\\Music".to_string()),
            FolderEntry::new("D:\\Other".to_string())
        ]
    );
}

#[test]
fn load_or_create_returns_config_error_on_invalid_yaml() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();
    fs::write(&paths.config_path, "::: not yaml :::").unwrap();

    let err = config_io::load_or_create(&paths).unwrap_err();
    match err {
        AppError::Config { message } => {
            assert!(
                message.contains("failed to parse"),
                "message was: {message}"
            );
            assert!(
                message.contains("config.yaml"),
                "message should mention file name; message was: {message}"
            );
        }
        other => panic!("expected AppError::Config, got {other:?}"),
    }
}

#[test]
fn load_or_create_returns_config_error_when_validation_fails() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    // Validation: supported_extensions must not be empty.
    let yaml = r#"
settings:
  supported_extensions: []
"#;
    fs::write(&paths.config_path, yaml).unwrap();

    let err = config_io::load_or_create(&paths).unwrap_err();
    match err {
        AppError::Config { message } => {
            assert!(message.contains("invalid"), "message was: {message}");
            assert!(
                message.contains("supported_extensions"),
                "message was: {message}"
            );
        }
        other => panic!("expected AppError::Config, got {other:?}"),
    }
}

#[test]
fn load_or_create_rejects_legacy_folders_vec_string_in_config_yaml() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    // Legacy config format used to allow `folders: ["C:\\Music", ...]`.
    // New stance: legacy compat is ONLY for playlists.yaml, not config.yaml.
    let yaml = r#"
schema_version: 1
settings:
  supported_extensions: [mp3, ogg]
folders: ["C:\\Music", "D:\\Other"]
"#;
    fs::write(&paths.config_path, yaml).unwrap();

    let err = config_io::load_or_create(&paths).unwrap_err();
    match err {
        AppError::Config { message } => {
            assert!(
                message.contains("failed to parse"),
                "message was: {message}"
            );
            assert!(
                message.contains("config.yaml"),
                "message should mention file name; message was: {message}"
            );
            assert!(
                message.to_lowercase().contains("folders")
                    || message.to_lowercase().contains("invalid type"),
                "message should hint at folders/type mismatch; message was: {message}"
            );
        }
        other => panic!("expected AppError::Config, got {other:?}"),
    }
}

#[test]
fn app_error_display_includes_path_for_io_errors() {
    let err = AppError::Io {
        path: PathBuf::from("X:\\some\\path"),
        source: std::io::Error::other("boom"),
    };
    let msg = err.to_string();
    assert!(msg.contains("I/O error at"), "msg was: {msg}");
    assert!(msg.contains("X:\\some\\path"), "msg was: {msg}");
    assert!(msg.contains("boom"), "msg was: {msg}");
}

#[test]
fn app_config_default_is_stable() {
    let cfg = AppConfig::default();
    assert_eq!(cfg.settings.min_size_kb, 1024);
    assert_eq!(cfg.settings.min_size_bytes, 1024 * 1024);
    assert!(!cfg.settings.shuffle);
    assert!(matches!(cfg.settings.repeat, RepeatMode::Off));
    assert!(cfg.folders.is_empty());
}
