use ost_player::config::io as config_io;
use ost_player::config::{effective_min_size_kb_for_folder, AppConfig, FolderEntry, ScanDepth};
use ost_player::paths::AppPaths;
use ost_player::playlists::io as playlists_io;
use ost_player::playlists::{Playlist, PlaylistsFile};
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
fn config_yaml_save_includes_custom_min_size_kb_when_present_and_omits_when_none() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let cfg = AppConfig {
        folders: vec![
            FolderEntry {
                path: "C:\\Default".to_string(),
                scan_depth: ScanDepth::RootOnly,
                custom_min_size_kb: None,
            },
            FolderEntry {
                path: "C:\\Custom".to_string(),
                scan_depth: ScanDepth::Recursive,
                custom_min_size_kb: Some(1234),
            },
        ],
        ..Default::default()
    };

    config_io::save(&paths, &cfg).expect("save should succeed");
    let v = read_yaml_value(&paths.config_path);
    let folders = v
        .get("folders")
        .and_then(Value::as_sequence)
        .expect("folders should be a sequence");
    assert_eq!(folders.len(), 2);

    let f0 = folders[0].as_mapping().expect("folder 0 should be mapping");
    assert!(
        f0.get(Value::from("custom_min_size_kb")).is_none(),
        "custom_min_size_kb must be omitted when None"
    );

    let f1 = folders[1].as_mapping().expect("folder 1 should be mapping");
    assert_eq!(
        f1.get(Value::from("custom_min_size_kb"))
            .and_then(Value::as_i64),
        Some(1234)
    );
}

#[test]
fn playlists_yaml_save_includes_custom_min_size_kb_when_present_and_omits_when_none() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let pls = PlaylistsFile {
        schema_version: 1,
        active: None,
        playlists: vec![Playlist {
            id: "p1".to_string(),
            name: "My Playlist".to_string(),
            folders: vec![
                FolderEntry {
                    path: "C:\\Default".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "C:\\Custom".to_string(),
                    scan_depth: ScanDepth::Recursive,
                    custom_min_size_kb: Some(4321),
                },
            ],
            extra: Default::default(),
        }],
        extra: Default::default(),
    };

    playlists_io::save(&paths, &pls).expect("save should succeed");
    let v = read_yaml_value(&paths.playlists_path);
    let playlists = v
        .get("playlists")
        .and_then(Value::as_sequence)
        .expect("playlists should be a sequence");
    assert_eq!(playlists.len(), 1);
    let p0 = playlists[0]
        .as_mapping()
        .expect("playlist should be mapping");
    let folders = p0
        .get(Value::from("folders"))
        .and_then(Value::as_sequence)
        .expect("folders should be a sequence");
    assert_eq!(folders.len(), 2);

    let f0 = folders[0].as_mapping().expect("folder 0 should be mapping");
    assert!(
        f0.get(Value::from("custom_min_size_kb")).is_none(),
        "custom_min_size_kb must be omitted when None"
    );

    let f1 = folders[1].as_mapping().expect("folder 1 should be mapping");
    assert_eq!(
        f1.get(Value::from("custom_min_size_kb"))
            .and_then(Value::as_i64),
        Some(4321)
    );
}

#[test]
fn normalized_config_clears_out_of_range_custom_min_size_kb_and_effective_kb_ignores_it() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
  min_size_kb: 111
  min_size_custom_kb_min: 10
  min_size_custom_kb_max: 10000
folders:
  - path: "C:\\TooSmall"
    custom_min_size_kb: 9
  - path: "C:\\TooBig"
    custom_min_size_kb: 10001
  - path: "C:\\Ok"
    custom_min_size_kb: 222
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    let normalized = cfg.clone().normalized();

    assert_eq!(normalized.folders.len(), 3);
    assert_eq!(normalized.folders[0].custom_min_size_kb, None);
    assert_eq!(normalized.folders[1].custom_min_size_kb, None);
    assert_eq!(normalized.folders[2].custom_min_size_kb, Some(222));

    // Even without normalization, the effective resolver should ignore out-of-range overrides.
    assert_eq!(
        effective_min_size_kb_for_folder(&cfg.folders[0], &cfg.settings),
        111
    );
    assert_eq!(
        effective_min_size_kb_for_folder(&cfg.folders[1], &cfg.settings),
        111
    );
    assert_eq!(
        effective_min_size_kb_for_folder(&cfg.folders[2], &cfg.settings),
        222
    );
}
