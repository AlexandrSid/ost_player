use ost_player::config::io as config_io;
use ost_player::config::{AppConfig, FolderEntry, ScanDepth};
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
fn yaml_parse_new_enum_form_accepts_all_variants() {
    for (yaml, want) in [
        ("root_only", ScanDepth::RootOnly),
        ("one_level", ScanDepth::OneLevel),
        ("recursive", ScanDepth::Recursive),
    ] {
        let raw = format!(
            r#"
path: 'C:\Music'
scan_depth: {yaml}
"#
        );
        let entry: FolderEntry = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(entry.scan_depth, want, "yaml was: {yaml}");
    }
}

#[test]
fn save_omits_scan_depth_when_default_and_includes_when_non_default() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    fs::create_dir_all(&paths.data_dir).unwrap();

    let cfg = AppConfig {
        folders: vec![
            FolderEntry {
                path: "C:\\RootOnly".to_string(),
                scan_depth: ScanDepth::RootOnly,
                custom_min_size_kb: None,
            },
            FolderEntry {
                path: "C:\\OneLevel".to_string(),
                scan_depth: ScanDepth::OneLevel,
                custom_min_size_kb: None,
            },
            FolderEntry {
                path: "C:\\Recursive".to_string(),
                scan_depth: ScanDepth::Recursive,
                custom_min_size_kb: None,
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
    assert_eq!(folders.len(), 3);

    let f0 = folders[0].as_mapping().expect("folder 0 should be mapping");
    assert!(
        f0.get(Value::from("scan_depth")).is_none(),
        "default scan_depth must be omitted on save"
    );

    let f1 = folders[1].as_mapping().expect("folder 1 should be mapping");
    assert_eq!(
        f1.get(Value::from("scan_depth")).and_then(Value::as_str),
        Some("one_level")
    );

    let f2 = folders[2].as_mapping().expect("folder 2 should be mapping");
    assert_eq!(
        f2.get(Value::from("scan_depth")).and_then(Value::as_str),
        Some("recursive")
    );
}
