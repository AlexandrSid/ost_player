use crate::config::{AppConfig, RepeatMode};
use crate::indexer::LibraryIndex;
use crate::paths::AppPaths;
use crate::player::PlayerSnapshot;
use crate::playlists::PlaylistsFile;
use crate::tui::action::Screen;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackSource {
    /// Playback queue was built while a playlist was active.
    Playlist(String),
    /// Playback queue was built directly from the current folders config.
    FoldersHash(u64),
}

impl PlaybackSource {
    pub fn from_active_playlist_or_folders(
        active_playlist_id: Option<&str>,
        folders: &[crate::config::FolderEntry],
    ) -> Self {
        if let Some(id) = active_playlist_id {
            return Self::Playlist(id.to_string());
        }

        let mut h = DefaultHasher::new();
        // We want identity based on the configured folders list. Include fields that affect
        // scanning results, and keep it stable across duplicate entries.
        let mut seen = std::collections::BTreeSet::<String>::new();
        for f in folders {
            if !seen.insert(f.path.clone()) {
                continue;
            }
            f.path.hash(&mut h);
            f.scan_depth.hash(&mut h);
            f.custom_min_size_kb.hash(&mut h);
        }
        Self::FoldersHash(h.finish())
    }
}

#[derive(Debug)]
pub struct AppState {
    pub paths: AppPaths,
    pub cfg: AppConfig,
    pub playlists: PlaylistsFile,
    pub playlists_dirty: bool,
    pub library: LibraryIndex,

    pub screen: Screen,
    pub status: Option<String>,
    pub last_error: Option<String>,
    pub player: PlayerSnapshot,
    pub playback_source: Option<PlaybackSource>,

    /// Basic UX: remember last selection per screen.
    pub main_selected_folder: usize,
    pub playlists_selected: usize,
}

impl AppState {
    pub fn new(
        paths: AppPaths,
        cfg: AppConfig,
        playlists: PlaylistsFile,
        library: LibraryIndex,
    ) -> Self {
        // Keep the initial UI snapshot consistent with config defaults.
        // The player thread will soon emit a real snapshot, but tests and early draws
        // should still render sensible values (e.g. default volume).
        let player = PlayerSnapshot {
            shuffle: cfg.settings.shuffle,
            repeat: cfg.settings.repeat,
            volume_percent: cfg.audio.volume_default_percent,
            ..Default::default()
        };
        Self {
            paths,
            cfg,
            playlists,
            playlists_dirty: false,
            library,
            screen: Screen::MainMenu,
            status: None,
            last_error: None,
            player,
            playback_source: None,
            main_selected_folder: 0,
            playlists_selected: 0,
        }
    }

    pub fn repeat_label(&self) -> &'static str {
        match self.cfg.settings.repeat {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::One => "one",
        }
    }
}
