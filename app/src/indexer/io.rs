use crate::error::{AppError, AppResult};
use crate::indexer::model::LibraryIndex;
use crate::paths::AppPaths;
use crate::persist;
use std::fs;
use std::path::PathBuf;

fn index_cache_path(paths: &AppPaths) -> PathBuf {
    paths.cache_dir.join("index.yaml")
}

pub fn load_best_effort(paths: &AppPaths) -> Option<LibraryIndex> {
    let path = index_cache_path(paths);
    if !path.exists() {
        // Try recovery if final missing.
        let _ = persist::recover_missing_final(&path);
    }
    if !path.exists() {
        return None;
    }

    let raw = fs::read_to_string(&path).ok()?;
    serde_yaml::from_str::<LibraryIndex>(&raw).ok()
}

pub fn save(paths: &AppPaths, index: &LibraryIndex) -> AppResult<()> {
    let path = index_cache_path(paths);
    let serialized = serde_yaml::to_string(index).map_err(|e| AppError::Config {
        message: format!("failed to serialize index cache: {e}"),
    })?;
    persist::write_text_safely(&path, &serialized)
}
