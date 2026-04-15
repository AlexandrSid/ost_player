use crate::error::AppResult;
use crate::tui::action::{Action, Screen};
use crate::tui::state::AppState;
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default)]
pub struct NowPlayingScreen;

impl NowPlayingScreen {
    pub fn on_tick(&mut self, _state: &AppState) -> AppResult<Option<Action>> {
        Ok(None)
    }

    pub fn on_key(&mut self, _state: &AppState, key: KeyEvent) -> AppResult<Option<Action>> {
        Ok(match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('m') => {
                Some(Action::Navigate(Screen::MainMenu))
            }
            KeyCode::Char(' ') | KeyCode::Enter => Some(Action::PlayerTogglePlayPause),
            KeyCode::Char('n') | KeyCode::Right => Some(Action::PlayerNext),
            KeyCode::Char('p') | KeyCode::Left => Some(Action::PlayerPrev),
            KeyCode::Char('x') => Some(Action::PlayerStop),
            KeyCode::Char('s') => Some(Action::ToggleShuffle),
            KeyCode::Char('r') => Some(Action::CycleRepeat),
            _ => None,
        })
    }

    pub fn on_paste(&mut self, _state: &AppState, _text: &str) -> AppResult<Option<Action>> {
        Ok(None)
    }
}
