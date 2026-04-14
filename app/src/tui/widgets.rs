use crossterm::event::{KeyCode, KeyEvent};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    cursor: usize, // grapheme index into `value` (0..=value.graphemes(true).count())
}

impl TextInput {
    pub fn new(title: &str, initial: &str, help: &str) -> Self {
        let cursor = initial.graphemes(true).count();
        Self {
            title: title.to_string(),
            help: help.to_string(),
            value: initial.to_string(),
            cursor,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<TextInputResult> {
        match key.code {
            KeyCode::Enter => Some(TextInputResult::Submit(self.value.clone())),
            KeyCode::Esc => Some(TextInputResult::Cancel),
            KeyCode::Backspace => {
                self.backspace();
                None
            }
            KeyCode::Left => {
                self.move_left();
                None
            }
            KeyCode::Right => {
                self.move_right();
                None
            }
            KeyCode::Char(c) => {
                // Keep input constrained to printable characters.
                if !c.is_control() {
                    self.insert_char(c);
                }
                None
            }
            _ => None,
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        let mut filtered = String::new();
        for c in text.chars() {
            if c.is_control() {
                continue;
            }
            filtered.push(c);
        }
        if filtered.is_empty() {
            return;
        }
        self.insert_str(&filtered);
    }

    pub fn display_for_width(&self, width: u16) -> (String, u16) {
        // `width` is the available content width (excluding borders).
        let width = width as usize;
        if width == 0 {
            return (String::new(), 0);
        }

        // Ensure cursor is within bounds even if `value` was modified externally.
        let total_graphemes = self.value.graphemes(true).count();
        let cursor = self.cursor.min(total_graphemes);

        // Precompute cumulative terminal cell widths for each grapheme boundary.
        // `cum_cells[i]` = total cells from start to grapheme index i.
        let mut cum_cells: Vec<usize> = Vec::with_capacity(total_graphemes + 1);
        cum_cells.push(0);
        for g in self.value.graphemes(true) {
            let w = UnicodeWidthStr::width(g);
            let next = cum_cells.last().copied().unwrap_or(0) + w;
            cum_cells.push(next);
        }

        let cursor_cells = cum_cells[cursor];
        let max_cursor_x = width.saturating_sub(1);

        // Choose a start grapheme index such that the cursor is always visible.
        // If it doesn't fit, scroll so the cursor lands at the rightmost visible column.
        let start_cells = cursor_cells.saturating_sub(max_cursor_x);
        let mut start_g = 0usize;
        // Find first grapheme boundary whose cumulative width is >= start_cells.
        // This intentionally tolerates zero-width chars.
        while start_g < total_graphemes && cum_cells[start_g] < start_cells {
            start_g += 1;
        }

        // Compute end grapheme index so visible width does not exceed `width`.
        let mut end_g = start_g;
        while end_g < total_graphemes && (cum_cells[end_g + 1] - cum_cells[start_g]) <= width {
            end_g += 1;
        }

        let start_byte = byte_index_for_grapheme_pos(&self.value, start_g);
        let end_byte = byte_index_for_grapheme_pos(&self.value, end_g);
        let visible = self.value.get(start_byte..end_byte).unwrap_or("").to_string();

        let cursor_x_cells = (cursor_cells - cum_cells[start_g]).min(max_cursor_x);
        (visible, cursor_x_cells as u16)
    }

    pub fn cursor_x_for_width(&self, width: u16) -> u16 {
        self.display_for_width(width).1
    }

    fn insert_char(&mut self, c: char) {
        self.clamp_cursor();
        let byte_idx = byte_index_for_grapheme_pos(&self.value, self.cursor);
        self.value.insert(byte_idx, c);
        self.cursor += c.to_string().graphemes(true).count();
    }

    fn insert_str(&mut self, s: &str) {
        self.clamp_cursor();
        let byte_idx = byte_index_for_grapheme_pos(&self.value, self.cursor);
        self.value.insert_str(byte_idx, s);
        self.cursor += s.graphemes(true).count();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.clamp_cursor();
        if self.cursor == 0 {
            return;
        }
        let start = byte_index_for_grapheme_pos(&self.value, self.cursor - 1);
        let end = byte_index_for_grapheme_pos(&self.value, self.cursor);
        self.value.drain(start..end);
        self.cursor -= 1;
    }

    fn move_left(&mut self) {
        self.clamp_cursor();
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.clamp_cursor();
        let total = self.value.graphemes(true).count();
        self.cursor = (self.cursor + 1).min(total);
    }

    fn clamp_cursor(&mut self) {
        let total = self.value.graphemes(true).count();
        if self.cursor > total {
            self.cursor = total;
        }
    }
}

fn byte_index_for_grapheme_pos(s: &str, grapheme_pos: usize) -> usize {
    if grapheme_pos == 0 {
        return 0;
    }

    // `grapheme_pos` can be == number of graphemes, in which case return s.len().
    match s.grapheme_indices(true).nth(grapheme_pos) {
        Some((byte_idx, _)) => byte_idx,
        None => s.len(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn text_input_insert_inserts_at_cursor_and_advances_cursor() {
        let mut ti = TextInput::new("t", "ac", "h");
        ti.on_key(key(KeyCode::Left));
        ti.on_key(key(KeyCode::Char('b')));

        assert_eq!(ti.value, "abc");
        assert_eq!(ti.cursor, 2, "cursor should advance past inserted char");
    }

    #[test]
    fn text_input_backspace_deletes_char_before_cursor_and_moves_cursor_left() {
        let mut ti = TextInput::new("t", "abc", "h");
        ti.on_key(key(KeyCode::Left)); // cursor after 'b'
        ti.on_key(key(KeyCode::Backspace)); // delete 'b'

        assert_eq!(ti.value, "ac");
        assert_eq!(ti.cursor, 1);
    }

    #[test]
    fn text_input_backspace_at_start_is_noop() {
        let mut ti = TextInput::new("t", "abc", "h");
        ti.cursor = 0;
        ti.on_key(key(KeyCode::Backspace));
        assert_eq!(ti.value, "abc");
        assert_eq!(ti.cursor, 0);
    }

    #[test]
    fn text_input_left_right_clamp_to_bounds() {
        let mut ti = TextInput::new("t", "abc", "h");

        ti.cursor = 0;
        ti.on_key(key(KeyCode::Left));
        assert_eq!(ti.cursor, 0);

        ti.cursor = ti.value.graphemes(true).count();
        ti.on_key(key(KeyCode::Right));
        assert_eq!(ti.cursor, ti.value.graphemes(true).count());
    }

    #[test]
    fn text_input_paste_inserts_at_cursor_not_at_end() {
        let mut ti = TextInput::new("t", "abef", "h");
        ti.cursor = 2; // after 'b'
        ti.on_paste("cd");

        assert_eq!(ti.value, "abcdef");
        assert_eq!(ti.cursor, 4, "cursor should advance by pasted text length");
    }

    #[test]
    fn text_input_display_for_width_returns_visible_slice_and_cursor_column_mapping() {
        let mut ti = TextInput::new("t", "0123456789", "h");
        ti.cursor = 8; // before '8'
        let width = 4u16;

        let (visible, cursor_x) = ti.display_for_width(width);

        // Visible slice should be at most `width` terminal cells.
        let visible_cells: usize = visible
            .chars()
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        assert!(visible_cells <= width as usize, "visible should be clamped to width cells");
        assert!(cursor_x < width, "cursor_x should be within 0..width-1");

        // Cursor should be within the window and mapped in terminal cells, not chars.
        let (visible2, cursor_x2) = ti.display_for_width(width);
        assert_eq!(visible, visible2);
        assert_eq!(cursor_x, cursor_x2);
    }

    #[test]
    fn text_input_editing_respects_utf8_char_boundaries() {
        // Includes multi-byte UTF-8 chars.
        let mut ti = TextInput::new("t", "aé😊b", "h");
        // Put cursor between 'é' and '😊' by moving left twice from end.
        ti.on_key(key(KeyCode::Left)); // before 'b'
        ti.on_key(key(KeyCode::Left)); // before '😊'
        ti.on_key(key(KeyCode::Char('X'))); // insert in the middle

        assert_eq!(ti.value, "aéX😊b");

        // Backspace should delete the 'X' (single-byte) and leave UTF-8 chars intact.
        ti.on_key(key(KeyCode::Backspace));
        assert_eq!(ti.value, "aé😊b");
    }

    #[test]
    fn text_input_display_for_width_accounts_for_wide_chars_in_cursor_x() {
        // "你" is typically width=2 in terminals.
        let mut ti = TextInput::new("t", "a你b", "h");
        // cursor at end
        ti.cursor = ti.value.graphemes(true).count();
        let (_visible, cursor_x) = ti.display_for_width(10);
        // cells: 'a'(1) + '你'(2) + 'b'(1) = 4
        assert_eq!(cursor_x, 4);
    }

    #[test]
    fn text_input_display_for_width_scrolls_by_cells_so_cursor_stays_visible() {
        // Emoji are typically width=2.
        let mut ti = TextInput::new("t", "a😊b😊c", "h");
        ti.cursor = ti.value.graphemes(true).count(); // end
        let width = 4u16;
        let (visible, cursor_x) = ti.display_for_width(width);
        assert!(cursor_x < width);

        let visible_cells: usize = visible
            .chars()
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        assert!(visible_cells <= width as usize);

        // Cursor at end should force window to include the last char.
        assert!(visible.ends_with('c'));
    }

    #[test]
    fn text_input_zwj_emoji_is_single_grapheme_for_cursor_and_backspace_and_width() {
        // 👩‍💻 is a ZWJ sequence (multiple chars, one grapheme cluster).
        let mut ti = TextInput::new("t", "a👩‍💻b", "h");
        assert_eq!(ti.value.graphemes(true).count(), 3, "a, 👩‍💻, b");

        // Cursor at end should be measured in graphemes.
        ti.cursor = ti.value.graphemes(true).count();

        // Move left once: should land before 'b' (i.e., after the 👩‍💻 grapheme).
        ti.on_key(key(KeyCode::Left));
        assert_eq!(ti.cursor, 2);

        // Backspace should delete the previous grapheme cluster (👩‍💻) in one go.
        ti.on_key(key(KeyCode::Backspace));
        assert_eq!(ti.value, "ab");
        assert_eq!(ti.cursor, 1);

        // Width/cursor_x should count the emoji cluster as a single display unit.
        let mut ti2 = TextInput::new("t", "a👩‍💻b", "h");
        ti2.cursor = ti2.value.graphemes(true).count();
        let (_visible, cursor_x) = ti2.display_for_width(20);
        let expected_cells =
            UnicodeWidthStr::width("a") + UnicodeWidthStr::width("👩‍💻") + UnicodeWidthStr::width("b");
        assert_eq!(cursor_x, expected_cells as u16);

        // Scrolling should never split the grapheme cluster.
        let (visible_small, _cx_small) = ti2.display_for_width(2);
        assert!(
            !visible_small.contains('‍'),
            "visible window must not slice into the ZWJ sequence"
        );
    }
}

