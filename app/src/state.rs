use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
use crate::persist;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastIndexSummary {
    pub tracks_total: usize,
    pub issues_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    #[serde(default = "StateFile::default_schema_version")]
    pub schema_version: u32,

    #[serde(default)]
    pub last_index: Option<LastIndexSummary>,
}

impl StateFile {
    fn default_schema_version() -> u32 {
        1
    }
}

impl Default for StateFile {
    fn default() -> Self {
        Self {
            schema_version: Self::default_schema_version(),
            last_index: None,
        }
    }
}

pub fn load_or_create(paths: &AppPaths) -> AppResult<StateFile> {
    let path = &paths.state_path;
    if !path.exists() {
        persist::recover_missing_final(path)?;
    }
    if !path.exists() {
        let s = StateFile::default();
        save(paths, &s)?;
        return Ok(s);
    }

    let raw = fs::read_to_string(path).map_err(|e| AppError::Io {
        path: path.clone(),
        source: e,
    })?;

    let s: StateFile = serde_yaml::from_str(&raw).map_err(|e| AppError::Config {
        message: format!("failed to parse `{}`: {e}", display_path(path)),
    })?;
    Ok(s)
}

pub fn save(paths: &AppPaths, state: &StateFile) -> AppResult<()> {
    let path = &paths.state_path;
    let serialized = serde_yaml::to_string(state).map_err(|e| AppError::Config {
        message: format!("failed to serialize state: {e}"),
    })?;
    persist::write_text_safely(path, &serialized)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
