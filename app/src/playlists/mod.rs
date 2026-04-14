pub mod io;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(default)]
    pub id: String,

    pub name: String,

    #[serde(default)]
    pub folders: Vec<String>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Playlist {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("playlist.name must not be empty".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistsFile {
    #[serde(default = "PlaylistsFile::default_schema_version")]
    pub schema_version: u32,

    /// Selected playlist by id (preferred) or by name (fallback for older configs).
    #[serde(default)]
    pub active: Option<String>,

    #[serde(default)]
    pub playlists: Vec<Playlist>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl PlaylistsFile {
    fn default_schema_version() -> u32 {
        1
    }

    pub fn validate(&self) -> Result<(), String> {
        for (idx, p) in self.playlists.iter().enumerate() {
            p.validate()
                .map_err(|e| format!("playlists[{idx}]: {e}"))?;
        }
        Ok(())
    }
}

impl Default for PlaylistsFile {
    fn default() -> Self {
        Self {
            schema_version: Self::default_schema_version(),
            active: None,
            playlists: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

