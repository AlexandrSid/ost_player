use crate::error::AppResult;
use crate::tui::action::{Action, Screen};
use crate::tui::state::AppState;
use crate::tui::widgets::{ConfirmDialog, TextInput};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default)]
pub struct PlaylistsScreen {
    create_input: Option<TextInput>,
    rename_input: Option<(usize, TextInput)>,
    confirm_delete: Option<ConfirmDialog>,
    confirm_overwrite: Option<ConfirmDialog>,
    confirm_load: Option<ConfirmDialog>,
}

impl PlaylistsScreen {
    pub fn on_tick(&mut self, _state: &AppState) -> AppResult<Option<Action>> {
        Ok(None)
    }

    pub fn on_key(&mut self, state: &AppState, key: KeyEvent) -> AppResult<Option<Action>> {
        if let Some(input) = &mut self.create_input {
            if let Some(done) = input.on_key(key) {
                self.create_input = None;
                return Ok(match done {
                    crate::tui::widgets::TextInputResult::Submit(v) => {
                        Some(Action::CreatePlaylist { name: v })
                    }
                    crate::tui::widgets::TextInputResult::Cancel => Some(Action::ClearStatus),
                });
            }
            return Ok(None);
        }

        if let Some((idx, input)) = &mut self.rename_input {
            if let Some(done) = input.on_key(key) {
                let idx = *idx;
                self.rename_input = None;
                return Ok(match done {
                    crate::tui::widgets::TextInputResult::Submit(v) => {
                        Some(Action::RenamePlaylist { idx, name: v })
                    }
                    crate::tui::widgets::TextInputResult::Cancel => Some(Action::ClearStatus),
                });
            }
            return Ok(None);
        }

        if let Some(confirm) = &mut self.confirm_delete {
            if let Some(res) = confirm.on_key(key) {
                self.confirm_delete = None;
                return Ok(match res {
                    true => Some(Action::DeletePlaylist {
                        idx: state.playlists_selected,
                    }),
                    false => Some(Action::ClearStatus),
                });
            }
            return Ok(None);
        }

        if let Some(confirm) = &mut self.confirm_overwrite {
            if let Some(res) = confirm.on_key(key) {
                self.confirm_overwrite = None;
                return Ok(match res {
                    true => Some(Action::OverwritePlaylistWithCurrent {
                        idx: state.playlists_selected,
                    }),
                    false => Some(Action::ClearStatus),
                });
            }
            return Ok(None);
        }

        if let Some(confirm) = &mut self.confirm_load {
            if let Some(res) = confirm.on_key(key) {
                self.confirm_load = None;
                return Ok(match res {
                    true => Some(Action::LoadPlaylist {
                        idx: state.playlists_selected,
                    }),
                    false => Some(Action::ClearStatus),
                });
            }
            return Ok(None);
        }

        Ok(self.handle_normal_key(state, key))
    }

    pub fn on_paste(&mut self, _state: &AppState, text: &str) -> AppResult<Option<Action>> {
        if let Some(input) = &mut self.create_input {
            input.on_paste(text);
            return Ok(None);
        }
        if let Some((_idx, input)) = &mut self.rename_input {
            input.on_paste(text);
            return Ok(None);
        }
        Ok(None)
    }

    fn handle_normal_key(&mut self, state: &AppState, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::Navigate(Screen::MainMenu)),
            KeyCode::Up => Some(Action::SelectPlaylistDelta(-1)),
            KeyCode::Down => Some(Action::SelectPlaylistDelta(1)),
            KeyCode::Char('s') => {
                if !state.playlists_dirty {
                    Some(Action::SetStatus("no playlist changes to save".to_string()))
                } else {
                    Some(Action::SavePlaylists)
                }
            }
            KeyCode::Char('n') => {
                self.create_input = Some(TextInput::new(
                    "Create playlist from current folders",
                    "",
                    "Type name  Enter=create  Esc=cancel",
                ));
                Some(Action::SetStatus("creating playlist...".to_string()))
            }
            KeyCode::Char('r') => {
                if state.playlists.playlists.is_empty() {
                    return Some(Action::SetStatus("no playlists to rename".to_string()));
                }
                let idx = state.playlists_selected;
                let initial = state
                    .playlists
                    .playlists
                    .get(idx)
                    .map(|p| p.name.as_str())
                    .unwrap_or("");
                self.rename_input = Some((
                    idx,
                    TextInput::new(
                        "Rename playlist",
                        initial,
                        "Type name  Enter=rename  Esc=cancel",
                    ),
                ));
                Some(Action::SetStatus("renaming playlist...".to_string()))
            }
            KeyCode::Char('d') => {
                if state.playlists.playlists.is_empty() {
                    return Some(Action::SetStatus("no playlists to delete".to_string()));
                }
                self.confirm_delete = Some(ConfirmDialog::new(
                    "Delete selected playlist?",
                    "Enter=yes  Esc=no",
                ));
                Some(Action::SetStatus("confirm delete...".to_string()))
            }
            KeyCode::Char('o') => {
                if state.playlists.playlists.is_empty() {
                    return Some(Action::SetStatus("no playlists to overwrite".to_string()));
                }
                self.confirm_overwrite = Some(ConfirmDialog::new(
                    "Overwrite selected playlist with current folders?",
                    "Enter=yes  Esc=no",
                ));
                Some(Action::SetStatus("confirm overwrite...".to_string()))
            }
            KeyCode::Char('l') | KeyCode::Enter => {
                if state.playlists.playlists.is_empty() {
                    return Some(Action::SetStatus("no playlists to load".to_string()));
                }
                let title = match state.player.status {
                    crate::player::PlaybackStatus::Stopped => {
                        "Load selected playlist (swap folders)?"
                    }
                    crate::player::PlaybackStatus::Playing
                    | crate::player::PlaybackStatus::Paused => {
                        "Stop playback and load selected playlist (swap folders)?"
                    }
                };
                self.confirm_load = Some(ConfirmDialog::new(title, "Enter=yes  Esc=no"));
                Some(Action::SetStatus("confirm load...".to_string()))
            }
            _ => None,
        }
    }

    pub fn view<'a>(&'a self, state: &'a AppState) -> PlaylistsView<'a> {
        PlaylistsView {
            create_input: self.create_input.as_ref(),
            rename_input: self.rename_input.as_ref().map(|(_, i)| i),
            confirm_delete: self.confirm_delete.as_ref(),
            confirm_overwrite: self.confirm_overwrite.as_ref(),
            confirm_load: self.confirm_load.as_ref(),
            playlists: &state.playlists.playlists,
            selected: state.playlists_selected,
            active_id: state.playlists.active.as_deref(),
        }
    }
}

pub struct PlaylistsView<'a> {
    pub create_input: Option<&'a TextInput>,
    pub rename_input: Option<&'a TextInput>,
    pub confirm_delete: Option<&'a ConfirmDialog>,
    pub confirm_overwrite: Option<&'a ConfirmDialog>,
    pub confirm_load: Option<&'a ConfirmDialog>,
    pub playlists: &'a [crate::playlists::Playlist],
    pub selected: usize,
    pub active_id: Option<&'a str>,
}
