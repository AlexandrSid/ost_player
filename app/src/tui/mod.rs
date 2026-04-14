mod terminal;
mod ui;

pub mod action;
pub mod app;
pub mod screens;
pub mod state;
pub mod widgets;

use crate::config::AppConfig;
use crate::error::AppResult;
use crate::paths::AppPaths;
use crate::playlists::PlaylistsFile;

pub fn run(paths: AppPaths, cfg: AppConfig, playlists: PlaylistsFile) -> AppResult<()> {
    let mut app = app::TuiApp::new(paths, cfg, playlists);
    terminal::run(&mut app)
}

