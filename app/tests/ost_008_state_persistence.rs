use ost_player::error::AppError;
use ost_player::paths::AppPaths;
use ost_player::state::{LastIndexSummary, StateFile};
use ost_player::{persist, state};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn make_paths_in(base_dir: PathBuf) -> AppPaths {
    let data_dir = base_dir.join("data");
    let config_path = data_dir.join("config.yaml");
    let cache_dir = data_dir.join("cache");
    let logs_dir = data_dir.join("logs");
    let playlists_dir = data_dir.join("playlists");
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

#[test]
fn load_or_create_creates_default_when_missing() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    let s = state::load_or_create(&paths).expect("missing state should be created");
    assert!(paths.state_path.is_file(), "state file should be created");
    assert_eq!(s.schema_version, 1);
    assert!(s.last_index.is_none());
}

#[test]
fn save_then_load_roundtrip_preserves_fields() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let s = StateFile {
        schema_version: 1,
        last_index: Some(LastIndexSummary {
            tracks_total: 123,
            issues_total: 7,
        }),
    };
    state::save(&paths, &s).expect("save should succeed");

    let loaded = state::load_or_create(&paths).expect("load should succeed");
    assert_eq!(loaded.schema_version, 1);
    assert_eq!(loaded.last_index.as_ref().unwrap().tracks_total, 123);
    assert_eq!(loaded.last_index.as_ref().unwrap().issues_total, 7);
}

#[test]
fn load_or_create_recovers_from_bak_when_final_missing() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let bak = persist::backup_path_for(&paths.state_path);
    let tmp = persist::temp_path_for(&paths.state_path);

    fs::write(
        &bak,
        r#"
schema_version: 1
last_index: { tracks_total: 9, issues_total: 1 }
"#,
    )
    .unwrap();
    fs::write(&tmp, "schema_version: 1\n").unwrap();

    // Ensure final missing, then load should restore from .bak first.
    assert!(!paths.state_path.exists());
    let s = state::load_or_create(&paths).unwrap();
    assert!(paths.state_path.exists(), "final should be restored");
    assert!(!bak.exists(), "bak should be moved into final");
    assert!(tmp.exists(), "tmp should remain untouched when bak was restored");
    assert_eq!(s.last_index.unwrap().tracks_total, 9);
}

#[test]
fn load_or_create_recovers_from_tmp_when_final_missing() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let tmp = persist::temp_path_for(&paths.state_path);
    fs::write(
        &tmp,
        r#"
schema_version: 1
last_index:
  tracks_total: 2
  issues_total: 0
"#,
    )
    .unwrap();

    let s = state::load_or_create(&paths).unwrap();
    assert!(paths.state_path.exists(), "final should be restored from tmp");
    assert!(!tmp.exists(), "tmp should be moved into final");
    assert_eq!(s.last_index.unwrap().tracks_total, 2);
}

#[test]
fn load_or_create_returns_config_error_on_invalid_yaml() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();
    fs::write(&paths.state_path, "::: not yaml :::").unwrap();

    let err = state::load_or_create(&paths).unwrap_err();
    match err {
        AppError::Config { message } => {
            assert!(message.contains("failed to parse"), "message was: {message}");
            assert!(
                message.contains("state.yaml"),
                "message should mention file name; message was: {message}"
            );
        }
        other => panic!("expected AppError::Config, got {other:?}"),
    }
}

