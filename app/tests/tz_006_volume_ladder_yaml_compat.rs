use ost_player::config::io as config_io;
use ost_player::config::AppConfig;
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
fn defaults_include_required_discrete_volume_ladder() {
    let cfg = AppConfig::default();
    let ladder = &cfg.audio.volume_available_percent;
    assert!(
        !ladder.is_empty(),
        "audio.volume_available_percent must not be empty by default"
    );
    assert_eq!(ladder.first().copied(), Some(0), "ladder must include 0");
    assert_eq!(ladder.last().copied(), Some(100), "ladder must include 100");

    let mut sorted = ladder.clone();
    sorted.sort();
    assert_eq!(
        ladder, &sorted,
        "audio.volume_available_percent must be sorted ascending"
    );

    let mut uniq = ladder.clone();
    uniq.dedup();
    assert_eq!(
        ladder, &uniq,
        "audio.volume_available_percent must contain unique values"
    );

    assert!(
        ladder.iter().all(|v| *v <= 100),
        "audio.volume_available_percent values must be within 0..=100"
    );

    // UX requirement: fine-grained levels at low volume, then step=5.
    let expected_prefix = vec![0u8, 1, 2, 3, 5, 7, 10, 13, 16, 20];
    assert_eq!(
        ladder
            .iter()
            .copied()
            .take(expected_prefix.len())
            .collect::<Vec<_>>(),
        expected_prefix
    );
    assert!(
        ladder.contains(&25) && ladder.contains(&30) && ladder.contains(&100),
        "expected the 5%-step region to be present (e.g. 25, 30, 100)"
    );
}

#[test]
fn config_load_or_create_persists_volume_ladder_and_new_field_names() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());

    let cfg = config_io::load_or_create(&paths).expect("should create default config");
    assert!(
        !cfg.audio.volume_available_percent.is_empty(),
        "defaults must persist a non-empty ladder"
    );

    let v = read_yaml_value(&paths.config_path);
    let audio = v
        .get("audio")
        .and_then(Value::as_mapping)
        .expect("audio should be a mapping");

    assert!(
        audio.get(Value::from("volume_available_percent")).is_some(),
        "config.yaml should persist audio.volume_available_percent"
    );
    assert!(
        audio.get(Value::from("volume_default_percent")).is_some(),
        "config.yaml should persist audio.volume_default_percent"
    );

    // Legacy fields should not be emitted.
    assert!(
        audio.get(Value::from("volume_step_percent")).is_none(),
        "config.yaml should not emit legacy audio.volume_step_percent"
    );
    assert!(
        audio.get(Value::from("default_volume_percent")).is_none(),
        "config.yaml should not emit legacy audio.default_volume_percent"
    );
}

#[test]
fn validate_rejects_invalid_volume_ladder_duplicate_unsorted_out_of_range_or_missing_endpoints() {
    let mut cfg = AppConfig::default();

    for (ladder, name) in [
        (vec![0u8, 10, 10, 100], "duplicate"),
        (vec![0u8, 10, 5, 100], "unsorted"),
        (vec![1u8, 5, 100], "missing 0"),
        (vec![0u8, 5, 99], "missing 100"),
        (vec![0u8, 5, 200, 100], "out of range"),
        (vec![], "empty"),
    ] {
        cfg.audio.volume_available_percent = ladder;
        let err = cfg.validate().unwrap_err();
        assert!(
            err.contains("audio.volume_available_percent"),
            "case {name}: expected error mentioning audio.volume_available_percent, got: {err}"
        );
    }
}

#[test]
fn yaml_migrates_volume_step_percent_into_generated_ladder_when_list_missing() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
audio:
  volume_step_percent: 7
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    assert_eq!(
        cfg.audio.volume_available_percent,
        vec![0u8, 7, 14, 21, 28, 35, 42, 49, 56, 63, 70, 77, 84, 91, 98, 100]
    );
}

#[test]
fn yaml_prefers_explicit_volume_available_percent_over_legacy_volume_step_percent_when_both_present(
) {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
audio:
  volume_step_percent: 25
  volume_available_percent: [0, 1, 2, 100]
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    assert_eq!(cfg.audio.volume_available_percent, vec![0u8, 1, 2, 100]);
}

#[test]
fn yaml_aliases_default_volume_percent_into_volume_default_percent() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
audio:
  default_volume_percent: 33
  volume_available_percent: [0, 33, 100]
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    assert_eq!(cfg.audio.volume_default_percent, 33);
}

#[test]
fn yaml_new_volume_default_percent_wins_when_both_new_and_legacy_present() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3]
audio:
  default_volume_percent: 33
  volume_default_percent: 44
  volume_available_percent: [0, 44, 100]
"#;
    let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
    assert_eq!(cfg.audio.volume_default_percent, 44);
}
