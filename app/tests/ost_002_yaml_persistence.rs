use ost_player::config::io as config_io;
use ost_player::config::{AppConfig, FolderEntry, RepeatMode};
use ost_player::error::AppError;
use ost_player::paths::AppPaths;
use ost_player::playlists::io as playlists_io;
use ost_player::playlists::PlaylistsFile;
use serde_yaml::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn make_paths_in(base_dir: PathBuf) -> AppPaths {
    let data_dir = base_dir.join("data");
    let cache_dir = data_dir.join("cache");
    let logs_dir = data_dir.join("logs");
    let playlists_dir = data_dir.join("playlists");
    let config_path = data_dir.join("config.yaml");
    let playlists_path = data_dir.join("playlists.yaml");
    let state_path = data_dir.join("state.yaml");
    AppPaths {
        base_dir,
        data_dir,
        cache_dir,
        logs_dir,
        playlists_dir,
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
fn app_paths_resolve_portable_layout_under_data_dir() {
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
        paths.playlists_dir,
        expected_base.join("data").join("playlists")
    );
    assert_eq!(paths.config_path, expected_base.join("data").join("config.yaml"));
    assert_eq!(
        paths.playlists_path,
        expected_base.join("data").join("playlists.yaml")
    );
    assert_eq!(paths.state_path, expected_base.join("data").join("state.yaml"));
}

#[test]
fn app_paths_ensure_writable_succeeds_in_temp_dir() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    paths.ensure_writable().expect("should be writable");
    assert!(paths.cache_dir.is_dir());
    assert!(paths.logs_dir.is_dir());
    assert!(paths.playlists_dir.is_dir());
}

#[test]
fn app_paths_ensure_writable_returns_io_error_if_data_dir_is_a_file() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    fs::write(&paths.data_dir, b"not a dir").unwrap();

    let err = paths.ensure_writable().unwrap_err();
    match err {
        AppError::Io { path, .. } => {
            assert_eq!(
                path,
                paths.cache_dir,
                "first create_dir_all should fail on cache_dir"
            );
        }
        other => panic!("expected AppError::Io, got {other:?}"),
    }
}

#[test]
fn config_load_or_create_creates_file_with_defaults() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    let cfg = config_io::load_or_create(&paths).expect("should create default config");
    assert!(paths.config_path.is_file(), "config file should be created");
    assert_eq!(cfg.settings.min_size_bytes, 1_000_000);
    assert!(!cfg.settings.shuffle);
    assert!(matches!(cfg.settings.repeat, RepeatMode::Off));
    assert_eq!(cfg.settings.supported_extensions, vec!["mp3".to_string(), "ogg".to_string()]);
}

#[test]
fn config_roundtrip_preserves_unknown_top_level_fields() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let yaml = r#"
schema_version: 1
settings:
  min_size_bytes: 1000000
  shuffle: false
  repeat: off
  supported_extensions: ["mp3", "ogg"]
folders: []
unknown_top_level:
  nested: 123
"#;
    fs::write(&paths.config_path, yaml).unwrap();

    let cfg = config_io::load_or_create(&paths).expect("should load with unknown fields preserved");
    config_io::save(&paths, &cfg).expect("save should succeed");

    let v = read_yaml_value(&paths.config_path);
    assert!(
        v.get("unknown_top_level").is_some(),
        "unknown_top_level should survive roundtrip"
    );
    assert_eq!(
        v.get("unknown_top_level")
            .and_then(|x| x.get("nested"))
            .and_then(Value::as_i64),
        Some(123)
    );
}

#[test]
fn config_save_is_atomicish_no_tmp_or_bak_left_and_no_data_loss() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let mut cfg = AppConfig::default();
    cfg.folders = vec![
        FolderEntry::new("C:\\Music".to_string()),
        FolderEntry::new("C:\\Music".to_string()),
    ];
    cfg.settings.shuffle = true;
    cfg.settings.repeat = RepeatMode::All;

    // Seed an existing file to exercise backup/replace code path.
    fs::write(&paths.config_path, "schema_version: 1\nsettings: { supported_extensions: [mp3, ogg] }\n")
        .unwrap();

    config_io::save(&paths, &cfg).expect("save should succeed");

    // No data loss: load what we just saved and compare key fields after normalization.
    let loaded = config_io::load_or_create(&paths).expect("should load saved config");
    assert!(loaded.settings.shuffle);
    assert!(matches!(loaded.settings.repeat, RepeatMode::All));
    assert_eq!(loaded.folders, vec![FolderEntry::new("C:\\Music".to_string())]);

    // Implementation writes via `{file}.tmp` and uses `{file}.bak` during replacement.
    let tmp = paths
        .config_path
        .with_file_name(format!(
            "{}.tmp",
            paths
                .config_path
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
    let bak = paths
        .config_path
        .with_file_name(format!(
            "{}.bak",
            paths
                .config_path
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
    assert!(!tmp.exists(), "tmp file should not remain");
    assert!(!bak.exists(), "bak file should not remain (removed best-effort)");
}

#[test]
fn playlists_load_or_create_creates_file_with_defaults() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    let pls = playlists_io::load_or_create(&paths).expect("should create default playlists");
    assert!(paths.playlists_path.is_file(), "playlists file should be created");
    assert_eq!(pls.schema_version, 1);
    assert!(pls.active.is_none());
    assert!(pls.playlists.is_empty());
}

#[test]
fn playlists_roundtrip_preserves_unknown_top_level_fields_and_playlist_fields() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let yaml = r#"
schema_version: 1
active: "p1"
unknown_root: true
playlists:
  - id: "p1"
    name: "My"
    folders: ["C:\\Music"]
    unknown_playlist_field: 42
"#;
    fs::write(&paths.playlists_path, yaml).unwrap();

    let pls = playlists_io::load_or_create(&paths).expect("load should succeed");
    playlists_io::save(&paths, &pls).expect("save should succeed");

    let v = read_yaml_value(&paths.playlists_path);
    assert!(
        v.get("unknown_root").is_some(),
        "unknown_root should survive roundtrip"
    );
    assert_eq!(v.get("unknown_root").and_then(Value::as_bool), Some(true));

    let playlists = v
        .get("playlists")
        .and_then(Value::as_sequence)
        .expect("playlists should be a sequence");
    let first = playlists.first().expect("one playlist expected");
    assert_eq!(
        first
            .get("unknown_playlist_field")
            .and_then(Value::as_i64),
        Some(42)
    );
}

#[test]
fn playlists_save_is_atomicish_no_tmp_or_bak_left_and_no_data_loss() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let mut pls = PlaylistsFile::default();
    pls.active = Some("p1".to_string());
    pls.playlists.push(ost_player::playlists::Playlist {
        id: "p1".to_string(),
        name: "My".to_string(),
        folders: vec![FolderEntry::new("C:\\Music".to_string())],
        extra: Default::default(),
    });

    fs::write(&paths.playlists_path, "schema_version: 1\nplaylists: []\n").unwrap();
    playlists_io::save(&paths, &pls).expect("save should succeed");

    let loaded = playlists_io::load_or_create(&paths).expect("should load saved playlists");
    assert_eq!(loaded.active.as_deref(), Some("p1"));
    assert_eq!(loaded.playlists.len(), 1);
    assert_eq!(loaded.playlists[0].name, "My");

    let tmp = paths
        .playlists_path
        .with_file_name(format!(
            "{}.tmp",
            paths
                .playlists_path
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
    let bak = paths
        .playlists_path
        .with_file_name(format!(
            "{}.bak",
            paths
                .playlists_path
                .file_name()
                .unwrap()
                .to_string_lossy()
        ));
    assert!(!tmp.exists(), "tmp file should not remain");
    assert!(!bak.exists(), "bak file should not remain (removed best-effort)");
}

