use crate::player::PlaybackStatus;
use crate::tui::action::Screen;
use crate::tui::app::TuiApp;
use crate::tui::scan_indicator::scan_mode_indicator_fixed;
use crate::tui::widgets::{ConfirmDialog, TextInput};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let state = &app.state;
    let outer = Block::default().title("OST Player").borders(Borders::ALL);
    let inner = outer.inner(frame.area());
    frame.render_widget(outer, frame.area());

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(2), Constraint::Length(2)].as_ref())
        .split(inner);

    match state.screen {
        Screen::MainMenu => draw_main_menu(frame, layout[0], app),
        Screen::Settings => draw_settings(frame, layout[0], app),
        Screen::Playlists => draw_playlists(frame, layout[0], app),
        Screen::Folders => {
            frame.render_widget(
                Paragraph::new("Folders screen (optional): not implemented yet.")
                    .wrap(Wrap { trim: false }),
                layout[0],
            );
        }
        Screen::NowPlaying => {
            draw_now_playing(frame, layout[0], app);
        }
    }

    draw_status_bar(frame, layout[1], app);
}

fn draw_main_menu(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let state = &app.state;
    let v = app.main_menu.view(state);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(area);

    let active_playlist = state
        .playlists
        .active
        .as_deref()
        .and_then(|id| state.playlists.playlists.iter().find(|p| p.id == id))
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "(none)".to_string());

    let header = format!(
        "Active playlist: {active_playlist}\nFolders: {}   Tracks indexed: {}   Issues: {}",
        state.cfg.folders.len(),
        state.library.tracks.len(),
        state.library.report.issues.len()
    );

    let mut items: Vec<ListItem> = Vec::new();
    if v.folders.is_empty() {
        items.push(ListItem::new("(no folders yet)"));
    } else {
        for (idx, f) in v.folders.iter().enumerate() {
            let sym = scan_mode_indicator_fixed(f.root_only);
            items.push(ListItem::new(format!("{:>2}. {sym} {}", idx + 1, f.path)));
        }
    }

    let list = List::new(items)
        .block(Block::default().title(header).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");

    let mut list_state = ratatui::widgets::ListState::default();
    if !v.folders.is_empty() {
        list_state.select(Some(
            v.selected_folder.min(v.folders.len().saturating_sub(1)),
        ));
    }
    frame.render_stateful_widget(list, cols[0], &mut list_state);

    let menu = [
        "Main menu:",
        "  1 / a  add folder",
        "  2 / d  remove selected folder",
        "  3 / t  toggle root_only for selected folder",
        "  4 / Enter / Space  play",
        "  5 / s  settings",
        "  6 / p  playlists",
        "  7 / r  rescan library",
        "  0 / q  exit",
        "",
        "Selection:",
        "  Up/Down",
    ]
    .join("\n");
    frame.render_widget(
        Paragraph::new(menu)
            .block(Block::default().title("Actions").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        cols[1],
    );

    if let Some(input) = v.add_folder {
        draw_text_input_modal(frame, input);
    } else if let Some(confirm) = v.confirm_remove {
        draw_confirm_modal(frame, confirm);
    }
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let state = &app.state;
    let v = app.settings.view();

    let text = format!(
        "Settings:\n\n  min_size_bytes: {}\n  shuffle: {}\n  repeat: {}\n\nKeys:\n  m  edit min_size_bytes\n  s  toggle shuffle\n  r  cycle repeat\n  Esc/q  back",
        state.cfg.settings.min_size_bytes,
        if state.cfg.settings.shuffle { "on" } else { "off" },
        state.repeat_label(),
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().title("Settings").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );

    if let Some(input) = v.min_size_input {
        draw_text_input_modal(frame, input);
    }
}

fn draw_playlists(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let state = &app.state;
    let v = app.playlists.view(state);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(area);

    let mut items: Vec<ListItem> = Vec::new();
    if v.playlists.is_empty() {
        items.push(ListItem::new("(no playlists yet)"));
    } else {
        for (idx, p) in v.playlists.iter().enumerate() {
            let active = v.active_id == Some(p.id.as_str());
            let prefix = if active { "* " } else { "  " };
            items.push(ListItem::new(format!(
                "{prefix}{:>2}. {}   (folders: {})",
                idx + 1,
                p.name,
                p.folders.len()
            )));
        }
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title("Playlists (* = active)")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");

    let mut list_state = ratatui::widgets::ListState::default();
    if !v.playlists.is_empty() {
        list_state.select(Some(v.selected.min(v.playlists.len().saturating_sub(1))));
    }
    frame.render_stateful_widget(list, cols[0], &mut list_state);

    let actions = [
        "Keys:",
        "  n  create (from current folders)",
        "  Enter/l  load (swap folders)",
        "  o  overwrite selected with current folders",
        "  r  rename selected",
        "  d  delete selected",
        "  Up/Down  select",
        "  Esc/q  back",
    ]
    .join("\n");
    frame.render_widget(
        Paragraph::new(actions)
            .block(Block::default().title("Actions").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        cols[1],
    );

    if let Some(input) = v.create_input.or(v.rename_input) {
        draw_text_input_modal(frame, input);
    } else if let Some(confirm) = v.confirm_delete.or(v.confirm_overwrite).or(v.confirm_load) {
        draw_confirm_modal(frame, confirm);
    }
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let state = &app.state;
    let status = state.status.clone().unwrap_or_else(|| match state.screen {
        Screen::MainMenu => "Ready. Choose an action.".to_string(),
        Screen::Settings => "Settings are auto-saved.".to_string(),
        Screen::Playlists => "Playlists are auto-saved.".to_string(),
        Screen::NowPlaying => "Now Playing.".to_string(),
        Screen::Folders => "Not implemented yet.".to_string(),
    });

    let text = format!(
        "{}    |    tracks={}  min_size={}  shuffle={}  repeat={}",
        status,
        state.library.tracks.len(),
        state.cfg.settings.min_size_bytes,
        if state.cfg.settings.shuffle {
            "on"
        } else {
            "off"
        },
        state.repeat_label()
    );

    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::TOP))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_now_playing(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let state = &app.state;
    let status = match state.player.status {
        PlaybackStatus::Stopped => "stopped",
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
    };

    let path = state
        .player
        .current_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "(none)".to_string());
    let name = state
        .player
        .current_path
        .as_ref()
        .and_then(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "(none)".to_string());

    let (pos_1, total) = match (state.player.queue_pos, state.player.queue_len) {
        (Some(pos), total) if total > 0 => (pos + 1, total),
        _ => (0, 0),
    };

    let track_pos = format_duration(state.player.track_position);
    let track_dur = state
        .player
        .track_duration
        .map(format_duration)
        .unwrap_or_else(|| "--:--".to_string());

    let last_error = state.last_error.as_deref().unwrap_or("(none)");

    let text = format!(
        "Now Playing\n\nStatus: {status}\nTrack:  {name}\nPath:   {path}\n\nQueue:  {pos_1}/{total}\nShuffle: {}\nRepeat: {}\nTime:   {track_pos} / {track_dur}\n\nLast error:\n  {last_error}\n\nKeys:\n  Space/Enter  play/pause\n  n / Right    next\n  p / Left     previous\n  x            stop\n  s            toggle shuffle\n  r            cycle repeat\n  Esc/q/m      back to main menu",
        if state.player.shuffle { "on" } else { "off" },
        match state.player.repeat {
            crate::config::RepeatMode::Off => "off",
            crate::config::RepeatMode::All => "all",
            crate::config::RepeatMode::One => "one",
        },
    );

    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().title("Now Playing").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn format_duration(d: std::time::Duration) -> String {
    let total = d.as_secs();
    let mm = total / 60;
    let ss = total % 60;
    format!("{mm:02}:{ss:02}")
}

fn draw_text_input_modal(frame: &mut Frame, input: &TextInput) {
    // 25% height is too small for typical 24-row terminals (it collapses the input row).
    // Use a taller modal so the bordered input field has a real content line.
    let area = centered_rect(70, 40, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL).title(""), area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Length(2),
            ]
            .as_ref(),
        )
        .margin(1)
        .split(area);

    frame.render_widget(
        Paragraph::new(input.title.as_str()).wrap(Wrap { trim: true }),
        inner[0],
    );

    let input_block = Block::default().borders(Borders::ALL).title("Input");
    let input_inner = input_block.inner(inner[1]);
    let (visible, cursor_x) = input.display_for_width(input_inner.width);
    frame.render_widget(
        Paragraph::new(visible)
            .block(input_block)
            .wrap(Wrap { trim: false }),
        inner[1],
    );

    // Ensure the terminal cursor is always visible while the modal is open.
    // Place it at the visible cursor position, even when buffer is empty.
    frame.set_cursor_position(Position {
        x: input_inner.x.saturating_add(cursor_x),
        y: input_inner.y,
    });
    frame.render_widget(
        Paragraph::new(input.help.as_str()).wrap(Wrap { trim: true }),
        inner[2],
    );
}

fn draw_confirm_modal(frame: &mut Frame, confirm: &ConfirmDialog) {
    // Keep confirm dialogs tall enough to avoid layout collapse on small terminals.
    let area = centered_rect(70, 30, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL).title(""), area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(2)].as_ref())
        .margin(1)
        .split(area);

    frame.render_widget(
        Paragraph::new(confirm.title.as_str()).wrap(Wrap { trim: true }),
        inner[0],
    );
    frame.render_widget(
        Paragraph::new(confirm.help.as_str()).wrap(Wrap { trim: true }),
        inner[1],
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, FolderEntry};
    use crate::paths::AppPaths;
    use crate::playlists::PlaylistsFile;
    use crate::tui::app::TuiApp;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
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

    fn buffer_as_text(buf: &Buffer) -> String {
        let mut out = String::new();
        let area = buf.area();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn text_input_modal_content_width(frame_area: Rect) -> u16 {
        let area = centered_rect(70, 40, frame_area);
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(2),
                    Constraint::Length(3),
                    Constraint::Length(2),
                ]
                .as_ref(),
            )
            .margin(1)
            .split(area);
        let input_block = Block::default().borders(Borders::ALL).title("Input");
        let input_inner = input_block.inner(inner[1]);
        input_inner.width
    }

    #[test]
    fn main_menu_actions_lines_begin_with_digit_key_labels() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();

        let app = TuiApp::new(paths, cfg, PlaylistsFile::default());

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let text = buffer_as_text(terminal.backend().buffer());

        // Stable, non-brittle checks:
        // - Find each action/help line by a distinctive phrase.
        // - Assert the line contains a digit key label (per UI-FIX-003 spec).
        // - Do not assert exact spacing or box borders.
        let expected_action_phrases = [
            "add folder",
            "remove selected folder",
            "toggle root_only",
            "play",
            "settings",
            "playlists",
            "rescan library",
            "exit",
        ];

        for phrase in expected_action_phrases {
            let line = text
                .lines()
                .find(|l| l.contains(phrase))
                .unwrap_or_else(|| {
                    panic!("expected to render an Actions line containing {phrase:?}")
                });
            let first = line
                .chars()
                .find(|c| c.is_ascii_digit())
                .unwrap_or_else(|| {
                    panic!("expected action line for {phrase:?} to contain a digit; got: {line:?}")
                });
            assert!(
                first.is_ascii_digit(),
                "expected action line for {phrase:?} to begin with a digit after trimming; got: {line:?}"
            );
        }
    }

    #[test]
    fn main_menu_folder_lines_render_root_only_symbol_before_path() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig {
            folders: vec![
                FolderEntry {
                    path: "C:\\Music".to_string(),
                    root_only: true,
                },
                FolderEntry {
                    path: "C:\\Games".to_string(),
                    root_only: false,
                },
            ],
            ..Default::default()
        };
        let folders = cfg.folders.clone();

        let mut app = TuiApp::new(paths, cfg, PlaylistsFile::default());
        app.state.main_selected_folder = 0; // selection adds "▶ " prefix; keep it predictable

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let text = buffer_as_text(terminal.backend().buffer());
        // Be robust to spacing/padding changes as long as:
        // - the symbol appears before the path on the same line
        // - the correct symbol is used for each mode
        // - the other mode's symbol does not appear on that same line (regression guard)
        let line_music = text
            .lines()
            .find(|l| l.contains("C:\\Music"))
            .expect("expected to render a line containing C:\\Music");
        let expected_root_only = scan_mode_indicator_fixed(true);
        let expected_recursive = scan_mode_indicator_fixed(false);
        assert!(
            line_music.contains(expected_root_only.as_str())
                && line_music.find(expected_root_only.as_str()) < line_music.find("C:\\Music"),
            "expected root_only=true indicator to appear before path; got: {line_music:?}"
        );
        assert!(
            !line_music.contains(expected_recursive.as_str()),
            "regression: root_only=true line must not contain recursive indicator; got: {line_music:?}"
        );

        let line_games = text
            .lines()
            .find(|l| l.contains("C:\\Games"))
            .expect("expected to render a line containing C:\\Games");
        assert!(
            line_games.contains(expected_recursive.as_str())
                && line_games.find(expected_recursive.as_str()) < line_games.find("C:\\Games"),
            "expected root_only=false indicator to appear before path; got: {line_games:?}"
        );
        assert!(
            !line_games.contains(expected_root_only.as_str()),
            "regression: root_only=false line must not contain root-only indicator; got: {line_games:?}"
        );

        // Regression guard: every rendered folder row must contain exactly one of the two indicators.
        for (idx, folder) in folders.iter().enumerate() {
            let line = text
                .lines()
                .find(|l| l.contains(folder.path.as_str()))
                .unwrap_or_else(|| {
                    panic!("expected to render a line containing {:?}", folder.path)
                });
            let want = scan_mode_indicator_fixed(folder.root_only);
            let other = scan_mode_indicator_fixed(!folder.root_only);
            assert!(
                line.contains(want.as_str()),
                "expected folder row {idx} to contain scan indicator {want:?}; got: {line:?}"
            );
            assert!(
                !line.contains(other.as_str()),
                "regression: folder row {idx} must not contain the other indicator {other:?}; got: {line:?}"
            );
        }
    }

    #[test]
    fn text_input_modal_renders_typed_text_in_input_field() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let input_value = "C:\\Music";
        let input = TextInput::new("Add folder", input_value, "Enter to submit");
        let expected_visible = input
            .display_for_width(text_input_modal_content_width(Rect::new(0, 0, 80, 24)))
            .0;

        terminal.draw(|f| draw_text_input_modal(f, &input)).unwrap();
        let text = buffer_as_text(terminal.backend().buffer());

        assert!(
            text.contains(expected_visible.as_str()),
            "expected modal to render visible input slice {expected_visible:?}; buffer was:\n{text}"
        );
        assert!(
            text.contains(input_value),
            "expected modal buffer to contain full input value {input_value:?} for this short case; buffer was:\n{text}"
        );
    }

    #[test]
    fn text_input_modal_renders_scrolled_visible_suffix_for_long_text() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        // Make the value longer than the input field so the modal must render a scrolled slice.
        let long_value = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let input = TextInput::new("Add folder", long_value, "Enter to submit");
        let width = text_input_modal_content_width(Rect::new(0, 0, 80, 24));
        let (expected_visible, _cursor_x) = input.display_for_width(width);

        terminal.draw(|f| draw_text_input_modal(f, &input)).unwrap();
        let text = buffer_as_text(terminal.backend().buffer());

        assert!(
            expected_visible.len() < long_value.len(),
            "test setup error: expected visible slice to be shorter than the full long value"
        );
        assert!(
            text.contains(expected_visible.as_str()),
            "expected modal to render scrolled visible slice {expected_visible:?}; buffer was:\n{text}"
        );
        assert!(
            expected_visible.ends_with('9'),
            "expected cursor-at-end to keep last character visible; got visible={expected_visible:?}"
        );
    }

    #[test]
    fn text_input_modal_renders_scaffolding_even_when_input_is_empty() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let input = TextInput::new("Add folder", "", "Enter to submit");
        terminal.draw(|f| draw_text_input_modal(f, &input)).unwrap();
        let text = buffer_as_text(terminal.backend().buffer());

        // No placeholder behavior exists yet; this just asserts the modal is present and labeled.
        assert!(
            text.contains("Add folder"),
            "expected title to render; buffer was:\n{text}"
        );
        assert!(
            text.contains("Enter to submit"),
            "expected help text to render; buffer was:\n{text}"
        );
        assert!(
            text.contains("Input"),
            "expected input box label; buffer was:\n{text}"
        );
    }

    #[test]
    fn confirm_modal_clears_background_under_modal_area() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let underlay = (0..24)
            .map(|_| "X".repeat(80))
            .collect::<Vec<_>>()
            .join("\n");

        let confirm = ConfirmDialog::new("Confirm remove?", "Enter=yes  Esc=no");
        terminal
            .draw(|f| {
                f.render_widget(
                    Paragraph::new(underlay.as_str()).wrap(Wrap { trim: false }),
                    f.area(),
                );
                draw_confirm_modal(f, &confirm);
            })
            .unwrap();

        let modal_area = centered_rect(70, 20, Rect::new(0, 0, 80, 24));
        let buf = terminal.backend().buffer();
        for y in modal_area.top()..modal_area.bottom() {
            for x in modal_area.left()..modal_area.right() {
                assert_ne!(
                    buf[(x, y)].symbol(),
                    "X",
                    "expected confirm modal to clear background; found underlay 'X' at ({x},{y}) in modal area"
                );
            }
        }
    }
}
