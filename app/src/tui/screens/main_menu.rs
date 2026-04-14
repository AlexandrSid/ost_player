use crate::error::AppResult;
use crate::tui::action::{Action, Screen};
use crate::tui::state::AppState;
use crate::tui::widgets::{ConfirmDialog, TextInput};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default)]
pub struct MainMenuScreen {
    add_folder: Option<TextInput>,
    confirm_remove: Option<ConfirmDialog>,
}

impl MainMenuScreen {
    pub fn on_tick(&mut self, _state: &AppState) -> AppResult<Option<Action>> {
        Ok(None)
    }

    pub fn on_key(&mut self, state: &AppState, key: KeyEvent) -> AppResult<Option<Action>> {
        if let Some(input) = &mut self.add_folder {
            if let Some(done) = input.on_key(key) {
                self.add_folder = None;
                return Ok(match done {
                    crate::tui::widgets::TextInputResult::Submit(v) => Some(Action::AddFolder(v)),
                    crate::tui::widgets::TextInputResult::Cancel => Some(Action::ClearStatus),
                });
            }
            return Ok(None);
        }

        if let Some(confirm) = &mut self.confirm_remove {
            if let Some(res) = confirm.on_key(key) {
                self.confirm_remove = None;
                return Ok(match res {
                    true => Some(Action::RemoveFolderAt(state.main_selected_folder)),
                    false => Some(Action::ClearStatus),
                });
            }
            return Ok(None);
        }

        Ok(self.handle_normal_key(state, key))
    }

    pub fn on_paste(&mut self, _state: &AppState, text: &str) -> AppResult<Option<Action>> {
        if let Some(input) = &mut self.add_folder {
            input.on_paste(text);
            return Ok(None);
        }
        Ok(None)
    }

    fn handle_normal_key(&mut self, state: &AppState, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('0') => Some(Action::Quit),
            KeyCode::Up => Some(Action::SelectFolderDelta(-1)),
            KeyCode::Down => Some(Action::SelectFolderDelta(1)),
            KeyCode::Char('1') | KeyCode::Char('a') => {
                self.add_folder = Some(TextInput::new(
                    "Add folder (absolute path)",
                    "",
                    "Enter=save  Esc=cancel",
                ));
                Some(Action::SetStatus("typing folder path...".to_string()))
            }
            KeyCode::Char('2') | KeyCode::Char('d') => {
                if state.cfg.folders.is_empty() {
                    return Some(Action::SetStatus("no folders to remove".to_string()));
                }
                self.confirm_remove = Some(ConfirmDialog::new(
                    "Remove selected folder?",
                    "Enter=yes  Esc=no",
                ));
                Some(Action::SetStatus("confirm removal...".to_string()))
            }
            KeyCode::Char('t') => {
                if state.cfg.folders.is_empty() {
                    Some(Action::SetStatus("no folders to toggle".to_string()))
                } else {
                    Some(Action::ToggleFolderRootOnlyAt(state.main_selected_folder))
                }
            }
            KeyCode::Char('3') | KeyCode::Enter | KeyCode::Char(' ') => {
                Some(Action::PlayerLoadFromLibrary { start_index: 0 })
            }
            KeyCode::Char('4') | KeyCode::Char('s') => Some(Action::Navigate(Screen::Settings)),
            KeyCode::Char('5') | KeyCode::Char('p') => Some(Action::Navigate(Screen::Playlists)),
            KeyCode::Char('6') | KeyCode::Char('r') => {
                if state.cfg.folders.is_empty() {
                    Some(Action::SetStatus("no folders configured to scan".to_string()))
                } else {
                    Some(Action::RescanLibrary)
                }
            }
            _ => None,
        }
    }

    pub fn view<'a>(&'a self, state: &'a AppState) -> MainMenuView<'a> {
        MainMenuView {
            add_folder: self.add_folder.as_ref(),
            confirm_remove: self.confirm_remove.as_ref(),
            folders: &state.cfg.folders,
            selected_folder: state.main_selected_folder,
        }
    }
}

pub struct MainMenuView<'a> {
    pub add_folder: Option<&'a TextInput>,
    pub confirm_remove: Option<&'a ConfirmDialog>,
    pub folders: &'a [crate::config::FolderEntry],
    pub selected_folder: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, FolderEntry};
    use crate::indexer::LibraryIndex;
    use crate::paths::AppPaths;
    use crate::playlists::PlaylistsFile;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    fn paths_for(base_dir: &Path) -> AppPaths {
        let base_dir = base_dir.to_path_buf();
        let data_dir = base_dir.join("data");
        AppPaths {
            base_dir,
            data_dir: data_dir.clone(),
            cache_dir: data_dir.join("cache"),
            logs_dir: data_dir.join("logs"),
            playlists_dir: data_dir.join("playlists"),
            config_path: data_dir.join("config.yaml"),
            playlists_path: data_dir.join("playlists.yaml"),
            state_path: data_dir.join("state.yaml"),
        }
    }

    #[test]
    fn key_t_emits_toggle_action_for_selected_folder_index() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.folders = vec![
            FolderEntry::new("A".to_string()),
            FolderEntry::new("B".to_string()),
        ];

        let mut state = AppState::new(paths, cfg, PlaylistsFile::default(), LibraryIndex::default());
        state.main_selected_folder = 1;

        let mut screen = MainMenuScreen::default();
        let action = screen
            .on_key(
                &state,
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::empty()),
            )
            .unwrap();

        assert_eq!(action, Some(Action::ToggleFolderRootOnlyAt(1)));
    }

    #[test]
    fn key_t_when_no_folders_sets_status_instead_of_toggling() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let state = AppState::new(paths, cfg, PlaylistsFile::default(), LibraryIndex::default());

        let mut screen = MainMenuScreen::default();
        let action = screen
            .on_key(
                &state,
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::empty()),
            )
            .unwrap();

        assert_eq!(
            action,
            Some(Action::SetStatus("no folders to toggle".to_string()))
        );
    }
}

