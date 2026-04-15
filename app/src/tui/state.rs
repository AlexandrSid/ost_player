use crate::config::{AppConfig, RepeatMode};
use crate::indexer::LibraryIndex;
use crate::paths::AppPaths;
use crate::player::PlayerSnapshot;
use crate::playlists::PlaylistsFile;
use crate::tui::action::Screen;

#[derive(Debug)]
pub struct AppState {
    pub paths: AppPaths,
    pub cfg: AppConfig,
    pub playlists: PlaylistsFile,
    pub library: LibraryIndex,

    pub screen: Screen,
    pub status: Option<String>,
    pub last_error: Option<String>,
    pub player: PlayerSnapshot,

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
        Self {
            paths,
            cfg,
            playlists,
            library,
            screen: Screen::MainMenu,
            status: None,
            last_error: None,
            player: PlayerSnapshot::default(),
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
