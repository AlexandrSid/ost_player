pub mod io;

use serde::{Deserialize, Serialize};
use serde::de;
use serde_yaml::Value;
use std::collections::BTreeMap;

use crate::config::FolderEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    #[serde(default)]
    pub id: String,

    pub name: String,

    /// Folder list for this playlist. Stored as objects to preserve flags like `root_only`,
    /// while remaining backward compatible with legacy `folders: ["/path", ...]` playlists.
    #[serde(default, deserialize_with = "deserialize_playlist_folders_compat")]
    pub folders: Vec<FolderEntry>,

    #[serde(flatten, default)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PlaylistFoldersCompat {
    Old(Vec<String>),
    New(Vec<FolderEntry>),
}

fn deserialize_playlist_folders_compat<'de, D>(deserializer: D) -> Result<Vec<FolderEntry>, D::Error>
where
    D: de::Deserializer<'de>,
{
    let v = Option::<PlaylistFoldersCompat>::deserialize(deserializer)?;
    Ok(match v {
        None => Vec::new(),
        Some(PlaylistFoldersCompat::Old(paths)) => paths.into_iter().map(FolderEntry::new).collect(),
        Some(PlaylistFoldersCompat::New(entries)) => entries,
    })
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

    #[serde(flatten, default)]
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

