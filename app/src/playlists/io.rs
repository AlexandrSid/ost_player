use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
use crate::persist;
use std::fs;
use std::path::Path;

use super::PlaylistsFile;

pub fn load_or_create(paths: &AppPaths) -> AppResult<PlaylistsFile> {
    let path = &paths.playlists_path;
    if !path.exists() {
        persist::recover_missing_final(path)?;
    }

    if !path.exists() {
        let pls = PlaylistsFile::default();
        save(paths, &pls)?;
        return Ok(pls);
    }

    let raw = fs::read_to_string(path).map_err(|e| AppError::Io {
        path: path.clone(),
        source: e,
    })?;

    let pls: PlaylistsFile = serde_yaml::from_str(&raw).map_err(|e| AppError::Config {
        message: format!("failed to parse `{}`: {e}", display_path(path)),
    })?;

    pls.validate().map_err(|msg| AppError::Config {
        message: format!("invalid `{}`: {msg}", display_path(path)),
    })?;

    Ok(pls)
}

pub fn save(paths: &AppPaths, pls: &PlaylistsFile) -> AppResult<()> {
    let path = &paths.playlists_path;
    let serialized = serde_yaml::to_string(pls).map_err(|e| AppError::Config {
        message: format!("failed to serialize playlists: {e}"),
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
        let bak = PathBuf::from(format!("{}.bak", paths.playlists_path.to_string_lossy()));
        std::fs::write(
            &bak,
            r#"
schema_version: 1
active: null
playlists:
  - id: p1
    name: My Playlist
    folders: []
"#,
        )
        .unwrap();

        let pls = load_or_create(&paths).unwrap();
        assert_eq!(pls.playlists.len(), 1);
        assert_eq!(pls.playlists[0].name, "My Playlist");
        assert!(paths.playlists_path.exists());
        assert!(!bak.exists());
    }

    #[test]
    fn load_or_create_restores_from_tmp_when_final_missing() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        std::fs::create_dir_all(paths.data_dir.clone()).unwrap();
        let tmp = PathBuf::from(format!("{}.tmp", paths.playlists_path.to_string_lossy()));
        std::fs::write(
            &tmp,
            r#"
schema_version: 1
active: null
playlists:
  - id: p1
    name: My Playlist
    folders: []
"#,
        )
        .unwrap();

        let pls = load_or_create(&paths).unwrap();
        assert_eq!(pls.playlists.len(), 1);
        assert_eq!(pls.playlists[0].name, "My Playlist");
        assert!(paths.playlists_path.exists());
        assert!(!tmp.exists());
    }

    #[test]
    fn legacy_playlists_folders_vec_string_deserializes_as_folder_entries_scan_depth_root_only() {
        let raw = r#"
schema_version: 1
active: null
playlists:
  - id: p1
    name: My Playlist
    folders: ["C:\\Music", "D:\\OST"]
"#;
        let pls: PlaylistsFile = serde_yaml::from_str(raw).unwrap();
        assert_eq!(pls.playlists.len(), 1);
        assert_eq!(pls.playlists[0].folders.len(), 2);
        assert_eq!(pls.playlists[0].folders[0].path, "C:\\Music");
        assert_eq!(
            pls.playlists[0].folders[0].scan_depth,
            crate::config::ScanDepth::RootOnly
        );
        assert_eq!(pls.playlists[0].folders[1].path, "D:\\OST");
        assert_eq!(
            pls.playlists[0].folders[1].scan_depth,
            crate::config::ScanDepth::RootOnly
        );
    }

    #[test]
    fn playlists_serialization_skips_default_scan_depth_but_keeps_non_default() {
        let pls = PlaylistsFile {
            schema_version: 1,
            active: None,
            playlists: vec![super::super::Playlist {
                id: "p1".to_string(),
                name: "My Playlist".to_string(),
                folders: vec![
                    crate::config::FolderEntry {
                        path: "C:\\Music".to_string(),
                        scan_depth: crate::config::ScanDepth::RootOnly,
                        custom_min_size_kb: None,
                    },
                    crate::config::FolderEntry {
                        path: "D:\\OST".to_string(),
                        scan_depth: crate::config::ScanDepth::Recursive,
                        custom_min_size_kb: None,
                    },
                ],
                extra: Default::default(),
            }],
            extra: Default::default(),
        };

        let y = serde_yaml::to_string(&pls).unwrap();
        assert!(!y.contains("scan_depth: root_only"));
        assert!(y.contains("scan_depth: recursive"));
    }
}
