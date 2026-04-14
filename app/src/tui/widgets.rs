use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputResult {
    Submit(String),
    Cancel,
}

#[derive(Debug, Clone)]
pub struct TextInput {
    pub title: String,
    pub help: String,
    pub value: String,
}

impl TextInput {
    pub fn new(title: &str, initial: &str, help: &str) -> Self {
        Self {
            title: title.to_string(),
            help: help.to_string(),
            value: initial.to_string(),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<TextInputResult> {
        match key.code {
            KeyCode::Enter => Some(TextInputResult::Submit(self.value.clone())),
            KeyCode::Esc => Some(TextInputResult::Cancel),
            KeyCode::Backspace => {
                self.value.pop();
                None
            }
            KeyCode::Char(c) => {
                // Keep input constrained to printable characters.
                if !c.is_control() {
                    self.value.push(c);
                }
                None
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub help: String,
}

impl ConfirmDialog {
    pub fn new(title: &str, help: &str) -> Self {
        Self {
            title: title.to_string(),
            help: help.to_string(),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => Some(true),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => Some(false),
            _ => None,
        }
    }
}

