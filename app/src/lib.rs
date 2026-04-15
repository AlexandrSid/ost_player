pub mod audio;
pub mod command_bus;
pub mod config;
pub mod error;
pub mod hotkeys;
pub mod indexer;
pub mod logging;
pub mod paths;
pub mod persist;
pub mod player;
pub mod playlists;
pub mod state;
pub mod tui;

#[cfg(windows)]
pub mod windows_icon;
