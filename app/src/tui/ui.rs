use crate::tui::action::Screen;
use crate::tui::app::TuiApp;
use crate::tui::widgets::{ConfirmDialog, TextInput};
use crate::player::PlaybackStatus;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
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
            let sym = if f.root_only { "↓" } else { "○" };
            items.push(ListItem::new(format!("{:>2}. {sym} {}", idx + 1, f.path)));
        }
    }

    let list = List::new(items)
        .block(Block::default().title(header).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");

    let mut list_state = ratatui::widgets::ListState::default();
    if !v.folders.is_empty() {
        list_state.select(Some(v.selected_folder.min(v.folders.len().saturating_sub(1))));
    }
    frame.render_stateful_widget(list, cols[0], &mut list_state);

    let menu = [
        "Main menu:",
        "  1 / a  add folder",
        "  2 / d  remove selected folder",
        "  t      toggle root_only for selected folder",
        "  3 / Enter / Space  play",
        "  4 / s  settings",
        "  5 / p  playlists",
        "  6 / r  rescan library",
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
    } else if let Some(confirm) = v.confirm_delete
        .or(v.confirm_overwrite)
        .or(v.confirm_load)
    {
        draw_confirm_modal(frame, confirm);
    }
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let state = &app.state;
    let status = state.status.clone().unwrap_or_else(|| {
        match state.screen {
            Screen::MainMenu => "Ready. Choose an action.".to_string(),
            Screen::Settings => "Settings are auto-saved.".to_string(),
            Screen::Playlists => "Playlists are auto-saved.".to_string(),
            Screen::NowPlaying => "Now Playing.".to_string(),
            Screen::Folders => "Not implemented yet.".to_string(),
        }
    });

    let text = format!(
        "{}    |    tracks={}  min_size={}  shuffle={}  repeat={}",
        status,
        state.library.tracks.len(),
        state.cfg.settings.min_size_bytes,
        if state.cfg.settings.shuffle { "on" } else { "off" },
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
        .and_then(|p| p.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()))
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

    let last_error = state
        .last_error
        .as_deref()
        .unwrap_or("(none)");

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
    let area = centered_rect(70, 25, frame.area());
    frame.render_widget(Block::default().borders(Borders::ALL).title(""), area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(3), Constraint::Length(2)].as_ref())
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
        Paragraph::new(visible).block(input_block),
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
    let area = centered_rect(70, 20, frame.area());
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

    #[test]
    fn main_menu_folder_lines_render_root_only_symbol_before_path() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![
            FolderEntry {
                path: "C:\\Music".to_string(),
                root_only: true,
            },
            FolderEntry {
                path: "C:\\Games".to_string(),
                root_only: false,
            },
        ];

        let mut app = TuiApp::new(paths, cfg, PlaylistsFile::default());
        app.state.main_selected_folder = 0; // selection adds "▶ " prefix; keep it predictable

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let text = buffer_as_text(terminal.backend().buffer());
        assert!(
            text.contains(". ↓ C:\\Music"),
            "expected root_only=true folder to render with ↓ symbol"
        );
        assert!(
            text.contains(". ○ C:\\Games"),
            "expected root_only=false folder to render with ○ symbol"
        );
    }
}

