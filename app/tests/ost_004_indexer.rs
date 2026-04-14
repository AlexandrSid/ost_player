use ost_player::indexer::io as index_io;
use ost_player::indexer::scan::scan_library;
use ost_player::indexer::{IndexIssueKind, ScanOptions};
use ost_player::paths::AppPaths;
use std::fs;
use std::path::{Path, PathBuf};
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

fn write_file_of_size(path: &Path, size: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, vec![b'x'; size]).unwrap();
}

fn default_scan_options() -> ScanOptions {
    ScanOptions {
        supported_extensions: vec!["mp3".to_string(), "ogg".to_string()],
        min_size_bytes: 0,
        allow_name_size_fallback_dedup: false,
        force_canonicalize_fail: false,
    }
}

#[test]
fn scan_finds_only_supported_extensions_case_insensitive() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("music");
    fs::create_dir_all(&root).unwrap();

    write_file_of_size(&root.join("a.mp3"), 10);
    write_file_of_size(&root.join("b.OGG"), 10);
    write_file_of_size(&root.join("c.flac"), 10);
    write_file_of_size(&root.join("d"), 10); // no extension

    let options = default_scan_options();
    let idx = scan_library(&[root.to_string_lossy().to_string()], &options);

    assert_eq!(idx.tracks.len(), 2, "only mp3/ogg should be included");
    let names = idx
        .tracks
        .iter()
        .map(|t| t.path.file_name().unwrap().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert!(names.iter().any(|n| n.eq_ignore_ascii_case("a.mp3")));
    assert!(names.iter().any(|n| n.eq_ignore_ascii_case("b.ogg")));
    assert_eq!(idx.report.skipped_ext, 2, "flac + no-ext should be skipped");
}

#[test]
fn scan_applies_min_size_bytes_filter() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("music");
    fs::create_dir_all(&root).unwrap();

    write_file_of_size(&root.join("small.mp3"), 5);
    write_file_of_size(&root.join("big.mp3"), 20);

    let mut options = default_scan_options();
    options.min_size_bytes = 10;

    let idx = scan_library(&[root.to_string_lossy().to_string()], &options);
    assert_eq!(idx.tracks.len(), 1);
    assert_eq!(
        idx.tracks[0]
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        "big.mp3"
    );
    assert_eq!(idx.report.skipped_small, 1);
}

#[test]
fn scan_dedups_when_same_root_scanned_twice_by_canonical_path() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("music");
    fs::create_dir_all(&root).unwrap();

    write_file_of_size(&root.join("a.mp3"), 10);
    write_file_of_size(&root.join("b.ogg"), 10);

    let options = default_scan_options();
    let roots = vec![
        root.to_string_lossy().to_string(),
        root.to_string_lossy().to_string(),
    ];
    let idx = scan_library(&roots, &options);

    assert_eq!(idx.tracks.len(), 2, "unique tracks should remain");
    assert_eq!(
        idx.report.deduped, 2,
        "second pass should dedup both files"
    );
    assert_eq!(idx.report.roots_total, 2);
    assert_eq!(idx.report.roots_ok, 2, "both roots were scanned successfully");
}

#[test]
fn scan_is_deterministically_sorted_by_rel_path_lowercased() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("music");
    fs::create_dir_all(&root).unwrap();

    // Intentionally create names that would be out of order by filesystem enumeration.
    write_file_of_size(&root.join("b.mp3"), 10);
    write_file_of_size(&root.join("A.mp3"), 10);
    write_file_of_size(&root.join("c.ogg"), 10);

    let options = default_scan_options();
    let idx = scan_library(&[root.to_string_lossy().to_string()], &options);
    assert_eq!(idx.tracks.len(), 3);

    let ordered = idx
        .tracks
        .iter()
        .map(|t| {
            t.rel_path
                .as_deref()
                .unwrap_or("<missing>")
                .replace('\\', "/")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ordered,
        vec!["A.mp3".to_string(), "b.mp3".to_string(), "c.ogg".to_string()]
    );
}

#[test]
fn scan_sort_is_total_and_deterministic_when_rel_path_ties() {
    let dir = tempdir().unwrap();
    let root_a = dir.path().join("music_a");
    let root_b = dir.path().join("music_b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    // Both roots contain the same rel_path; primary sort key will tie.
    write_file_of_size(&root_a.join("same.mp3"), 10);
    write_file_of_size(&root_b.join("same.mp3"), 10);

    let options = default_scan_options();
    let idx = scan_library(
        &[
            root_b.to_string_lossy().to_string(),
            root_a.to_string_lossy().to_string(),
        ],
        &options,
    );

    assert_eq!(idx.tracks.len(), 2);

    // With tie-breakers, ordering is deterministic (rel_path then absolute path bytes/wide).
    let paths = idx
        .tracks
        .iter()
        .map(|t| t.path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

#[test]
fn scan_fallback_dedup_does_not_collide_across_roots() {
    let dir = tempdir().unwrap();
    let root_a = dir.path().join("music_a");
    let root_b = dir.path().join("music_b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();

    // Same rel path + same size in different roots must NOT be deduped.
    write_file_of_size(&root_a.join("same.mp3"), 10);
    write_file_of_size(&root_b.join("same.mp3"), 10);

    let mut options = default_scan_options();
    options.allow_name_size_fallback_dedup = true;
    options.force_canonicalize_fail = true;

    let idx = scan_library(
        &[
            root_a.to_string_lossy().to_string(),
            root_b.to_string_lossy().to_string(),
        ],
        &options,
    );

    assert_eq!(idx.tracks.len(), 2);
    assert_eq!(idx.report.deduped, 0);
}

#[test]
fn scan_records_structured_issue_for_missing_folder() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");

    let options = default_scan_options();
    let idx = scan_library(&[missing.to_string_lossy().to_string()], &options);

    assert_eq!(idx.tracks.len(), 0);
    assert_eq!(idx.report.roots_total, 1);
    assert_eq!(idx.report.roots_ok, 0);
    assert!(
        idx.report
            .issues
            .iter()
            .any(|i| i.kind == IndexIssueKind::MissingFolder),
        "should include MissingFolder issue"
    );
    assert_eq!(
        idx.report.issue_counts.get(&IndexIssueKind::MissingFolder),
        Some(&1)
    );
}

#[test]
fn scan_records_structured_issue_when_root_is_not_a_directory_read_dir_failed() {
    let dir = tempdir().unwrap();
    let root_file = dir.path().join("not_a_dir");
    write_file_of_size(&root_file, 10);

    let options = default_scan_options();
    let idx = scan_library(&[root_file.to_string_lossy().to_string()], &options);

    assert_eq!(idx.report.roots_total, 1);
    assert_eq!(idx.report.roots_ok, 0);
    assert_eq!(idx.tracks.len(), 0);
    assert!(
        idx.report
            .issues
            .iter()
            .any(|i| i.kind == IndexIssueKind::ReadDirFailed || i.kind == IndexIssueKind::MissingFolder),
        "should include a structured issue for root read_dir failure"
    );
    assert!(
        idx.report.issue_counts.get(&IndexIssueKind::ReadDirFailed).is_some()
            || idx.report.issue_counts.get(&IndexIssueKind::MissingFolder).is_some()
    );
}

#[test]
fn index_cache_save_load_roundtrip_best_effort() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    paths.ensure_data_dirs().unwrap();

    let root = dir.path().join("music");
    fs::create_dir_all(&root).unwrap();
    write_file_of_size(&root.join("a.mp3"), 12);
    write_file_of_size(&root.join("b.ogg"), 34);

    let options = default_scan_options();
    let idx = scan_library(&[root.to_string_lossy().to_string()], &options);
    index_io::save(&paths, &idx).expect("save index cache should succeed");

    let loaded = index_io::load_best_effort(&paths).expect("cache should load after save");
    assert_eq!(loaded.schema_version, idx.schema_version);
    assert_eq!(loaded.tracks.len(), idx.tracks.len());
    assert_eq!(loaded.report.tracks_total, idx.report.tracks_total);

    for (a, b) in loaded.tracks.iter().zip(idx.tracks.iter()) {
        assert_eq!(a.path, b.path);
        assert_eq!(a.rel_path, b.rel_path);
        assert_eq!(a.size_bytes, b.size_bytes);
        assert_eq!(a.id, b.id);
    }
}

#[test]
fn index_cache_load_best_effort_returns_none_when_missing() {
    let dir = tempdir().unwrap();
    let paths = make_paths_in(dir.path().to_path_buf());
    paths.ensure_data_dirs().unwrap();

    let loaded = index_io::load_best_effort(&paths);
    assert!(loaded.is_none());
}

