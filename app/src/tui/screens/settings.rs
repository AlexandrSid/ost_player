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
                            "min_size_bytes must be an integer".to_string(),
                        )))
                    }
                };
                return Ok(Some(Action::SetMinSizeBytes(parsed)));
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
                    "Set min_size_bytes",
                    &state.cfg.settings.min_size_bytes.to_string(),
                    "Type number  Enter=save  Esc=cancel",
                ));
                Some(Action::SetStatus("editing min_size_bytes...".to_string()))
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
