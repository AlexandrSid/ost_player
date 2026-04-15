use crate::error::AppResult;
use crate::tui::action::{Action, Screen};
use crate::tui::state::AppState;
use crate::tui::widgets::TextInput;
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default)]
pub struct SettingsScreen {
    min_size_input: Option<TextInput>,
}

impl SettingsScreen {
    pub fn on_tick(&mut self, _state: &AppState) -> AppResult<Option<Action>> {
        Ok(None)
    }

    pub fn on_key(&mut self, state: &AppState, key: KeyEvent) -> AppResult<Option<Action>> {
        if let Some(input) = &mut self.min_size_input {
            if let Some(done) = input.on_key(key) {
                let v = match done {
                    crate::tui::widgets::TextInputResult::Submit(v) => v,
                    crate::tui::widgets::TextInputResult::Cancel => {
                        self.min_size_input = None;
                        return Ok(Some(Action::ClearStatus));
                    }
                };
                self.min_size_input = None;
                let parsed: u64 = match v.trim().parse() {
                    Ok(n) => n,
                    Err(_) => {
                        return Ok(Some(Action::SetStatus(
                            "min_size_kb must be an integer".to_string(),
                        )))
                    }
                };
                return Ok(Some(Action::SetMinSizeKb(parsed)));
            }
            return Ok(None);
        }

        Ok(self.handle_normal_key(state, key))
    }

    pub fn on_paste(&mut self, _state: &AppState, text: &str) -> AppResult<Option<Action>> {
        if let Some(input) = &mut self.min_size_input {
            input.on_paste(text);
            return Ok(None);
        }
        Ok(None)
    }

    fn handle_normal_key(&mut self, state: &AppState, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::Navigate(Screen::MainMenu)),
            KeyCode::Char('m') => {
                self.min_size_input = Some(TextInput::new(
                    "Set min_size_kb",
                    &state.cfg.settings.min_size_kb.to_string(),
                    "Type number  Enter=save  Esc=cancel",
                ));
                Some(Action::SetStatus("editing min_size_kb...".to_string()))
            }
            KeyCode::Char('s') => Some(Action::ToggleShuffle),
            KeyCode::Char('r') => Some(Action::CycleRepeat),
            _ => None,
        }
    }

    pub fn view(&self) -> SettingsView<'_> {
        SettingsView {
            min_size_input: self.min_size_input.as_ref(),
        }
    }
}

pub struct SettingsView<'a> {
    pub min_size_input: Option<&'a TextInput>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::indexer::LibraryIndex;
    use crate::paths::AppPaths;
    use crate::playlists::PlaylistsFile;
    use crate::tui::state::AppState;
    use crossterm::event::KeyModifiers;

    fn paths_for(dir: &std::path::Path) -> AppPaths {
        let base_dir = dir.to_path_buf();
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn state_with_min_size_kb(td: &tempfile::TempDir, min_size_kb: u64) -> AppState {
        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = min_size_kb;
        cfg.settings.min_size_bytes = min_size_kb * 1024;
        AppState::new(
            paths_for(td.path()),
            cfg,
            PlaylistsFile::default(),
            LibraryIndex::default(),
        )
    }

    #[test]
    fn editing_min_size_kb_submits_set_min_size_kb_action_not_bytes() {
        let td = tempfile::tempdir().unwrap();
        let state = state_with_min_size_kb(&td, 0);
        let mut s = SettingsScreen::default();

        // Enter edit mode.
        let a = s.on_key(&state, key(KeyCode::Char('m'))).unwrap().unwrap();
        assert_eq!(a, Action::SetStatus("editing min_size_kb...".to_string()));
        assert!(s.view().min_size_input.is_some());

        // Clear the initial "0" then type "123" and submit.
        s.on_key(&state, key(KeyCode::Backspace)).unwrap();
        s.on_key(&state, key(KeyCode::Char('1'))).unwrap();
        s.on_key(&state, key(KeyCode::Char('2'))).unwrap();
        s.on_key(&state, key(KeyCode::Char('3'))).unwrap();

        let a = s.on_key(&state, key(KeyCode::Enter)).unwrap().unwrap();
        assert_eq!(a, Action::SetMinSizeKb(123));
        assert!(
            s.view().min_size_input.is_none(),
            "input should close on submit"
        );
    }

    #[test]
    fn editing_min_size_kb_invalid_input_returns_status_error_and_closes_modal() {
        let td = tempfile::tempdir().unwrap();
        let state = state_with_min_size_kb(&td, 0);
        let mut s = SettingsScreen::default();

        s.on_key(&state, key(KeyCode::Char('m'))).unwrap();
        assert!(s.view().min_size_input.is_some());

        // Clear "0", type invalid number, submit.
        s.on_key(&state, key(KeyCode::Backspace)).unwrap();
        s.on_key(&state, key(KeyCode::Char('1'))).unwrap();
        s.on_key(&state, key(KeyCode::Char('2'))).unwrap();
        s.on_key(&state, key(KeyCode::Char('a'))).unwrap();

        let a = s.on_key(&state, key(KeyCode::Enter)).unwrap().unwrap();
        assert_eq!(
            a,
            Action::SetStatus("min_size_kb must be an integer".to_string())
        );
        assert!(
            s.view().min_size_input.is_none(),
            "input should close on submit"
        );
    }
}
