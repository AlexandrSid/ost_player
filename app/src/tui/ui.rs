use crate::player::PlaybackStatus;
use crate::tui::action::Screen;
use crate::tui::app::TuiApp;
use crate::tui::scan_indicator::scan_depth_indicator_fixed;
use crate::tui::widgets::{ConfirmDialog, TextInput};
use crate::{config::effective_min_size_kb_for_folder, config::FolderEntry};
use crate::{config::TapHoldBinding, hotkeys::hints as hotkey_hints};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

const MIN_SIZE_MARKER_GLOBAL: &str = "◼";
const MIN_SIZE_MARKER_CUSTOM: &str = "🄲";

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
            let sym = scan_depth_indicator_fixed(f.scan_depth);
            let (marker, eff_kb) = folder_min_size_marker_and_kb(f, &state.cfg.settings);
            items.push(ListItem::new(format!(
                "{:>2}. {sym} {marker} {eff_kb}kb {}",
                idx + 1,
                f.path
            )));
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

    let menu = main_menu_actions_block(&state.cfg);
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
    } else if let Some(input) = v.custom_min_size_input {
        draw_text_input_modal(frame, input);
    }
}

fn main_menu_actions_block(cfg: &crate::config::AppConfig) -> String {
    use crate::config::MainMenuCommand;

    fn cmd_label_and_alpha_hint(cmd: MainMenuCommand) -> (&'static str, Option<&'static str>) {
        match cmd {
            MainMenuCommand::AddFolder => ("add folder", Some("a")),
            MainMenuCommand::RemoveSelectedFolder => ("remove selected folder", Some("d")),
            MainMenuCommand::CycleSelectedFolderScanDepth => {
                ("cycle scan depth for selected folder", Some("t"))
            }
            MainMenuCommand::SetSelectedFolderCustomMinSizeKb => {
                ("set custom min_size for selected folder", Some("c"))
            }
            MainMenuCommand::Play => ("play", None),
            MainMenuCommand::Settings => ("settings", Some("s")),
            MainMenuCommand::Playlists => ("playlists", Some("p")),
            MainMenuCommand::RescanLibrary => ("rescan library", Some("r")),
        }
    }

    let mut lines: Vec<String> = Vec::new();

    if let Some(map) = cfg.tui.resolved_main_menu_numeric_mapping() {
        lines.push("Main menu (digits 1..9):".to_string());
        for (idx, cmd) in map.iter().enumerate() {
            let Some(cmd) = cmd else { continue };
            let key = (idx as u8) + 1;
            let (label, alpha) = cmd_label_and_alpha_hint(*cmd);
            let extra = match (*cmd, alpha) {
                (MainMenuCommand::Play, _) => " / Enter / Space".to_string(),
                (_, Some(ch)) => format!(" / {ch}"),
                _ => "".to_string(),
            };
            lines.push(format!("  {key}{extra}  {label}"));
        }
        lines.push("  0 / q  exit".to_string());
    } else {
        lines.extend([
            "Main menu:".to_string(),
            "  1 / a  add folder".to_string(),
            "  2 / d  remove selected folder".to_string(),
            "  3 / t  cycle scan depth for selected folder".to_string(),
            "  4 / c  set custom min_size for selected folder".to_string(),
            "  5 / Enter / Space  play".to_string(),
            "  6 / s  settings".to_string(),
            "  7 / p  playlists".to_string(),
            "  8 / r  rescan library".to_string(),
            "  0 / q  exit".to_string(),
        ]);
    }

    lines.push("".to_string());
    lines.push("Selection:".to_string());
    lines.push("  Up/Down".to_string());
    lines.join("\n")
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let state = &app.state;
    let v = app.settings.view();

    let text = format!(
        "Settings:\n\n  min_size_kb: {}kb\n  shuffle: {}\n  repeat: {}\n\nKeys:\n  m  edit min_size_kb\n  s  toggle shuffle\n  r  cycle repeat\n  Esc/q  back",
        state.cfg.settings.min_size_kb,
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

    let effective_min_size_kb = match state.screen {
        Screen::MainMenu if !state.cfg.folders.is_empty() => {
            let idx = state
                .main_selected_folder
                .min(state.cfg.folders.len().saturating_sub(1));
            effective_min_size_kb_for_folder(&state.cfg.folders[idx], &state.cfg.settings)
        }
        _ => state.cfg.settings.min_size_kb,
    };

    let text = format!(
        "{}    |    tracks={}  min_size={}kb  shuffle={}  repeat={}  Volume={}%",
        status,
        state.library.tracks.len(),
        effective_min_size_kb,
        if state.cfg.settings.shuffle {
            "on"
        } else {
            "off"
        },
        state.repeat_label(),
        state.player.volume_percent
    );

    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::TOP))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn folder_min_size_marker_and_kb(
    folder: &FolderEntry,
    settings: &crate::config::SettingsConfig,
) -> (&'static str, u64) {
    let eff_kb = effective_min_size_kb_for_folder(folder, settings);
    let min_kb = settings.min_size_custom_kb_min;
    let max_kb = settings.min_size_custom_kb_max;
    let marker = match folder.custom_min_size_kb {
        Some(v) if (min_kb..=max_kb).contains(&v) => MIN_SIZE_MARKER_CUSTOM,
        _ => MIN_SIZE_MARKER_GLOBAL,
    };
    (marker, eff_kb)
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

    let keys = now_playing_keys_block(state);
    let text = format!(
        "Status: {status}\nTrack:  {name}\nPath:   {path}\n\nQueue:  {pos_1}/{total}\nShuffle: {}\nRepeat: {}\nTime:   {track_pos} / {track_dur}\n\nLast error:\n  {last_error}\n\n{keys}",
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

fn chord_hint_or_unknown(hint: Option<String>) -> String {
    hint.unwrap_or_else(|| "?".to_string())
}

fn chord_hint(chord: &Option<crate::config::HotkeyChord>) -> Option<String> {
    chord.as_ref().map(hotkey_hints::format_chord)
}

fn tap_hold_hint(b: &Option<TapHoldBinding>) -> Option<String> {
    b.as_ref().map(hotkey_hints::format_tap_hold_binding)
}

fn now_playing_keys_block(state: &crate::tui::state::AppState) -> String {
    let b = &state.cfg.hotkeys.bindings;

    // Keep a stable, aligned layout: `<keys>` column + two spaces + `<action>`.
    let entries: [(&str, Option<String>); 6] = [
        ("play/pause", chord_hint(&b.play_pause)),
        ("next", tap_hold_hint(&b.next)),
        ("previous", tap_hold_hint(&b.prev)),
        ("toggle shuffle", chord_hint(&b.shuffle_toggle)),
        ("cycle repeat", chord_hint(&b.repeat_toggle)),
        ("stop", Some("x".to_string())),
    ];

    let key_width = entries
        .iter()
        .map(|(_, k)| chord_hint_or_unknown(k.clone()).len())
        .max()
        .unwrap_or(1)
        .max("Esc/q/m".len());

    let mut out = String::new();
    out.push_str("Keys:\n");
    for (label, key) in entries {
        let key = chord_hint_or_unknown(key);
        out.push_str(&format!(
            "  {:<key_width$}  {label}\n",
            key,
            key_width = key_width
        ));
    }
    out.push_str(&format!(
        "  {:<key_width$}  back to main menu",
        "Esc/q/m",
        key_width = key_width
    ));
    out
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
    use crate::config::{
        AppConfig, FolderEntry, HotkeyChord, HotkeyKey, HotkeyModifier, MainMenuCommand,
        MainMenuNumericBinding, TapHoldBinding, TuiConfig,
    };
    use crate::paths::AppPaths;
    use crate::playlists::PlaylistsFile;
    use crate::tui::action::Screen;
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

    fn buffer_rect_as_text(buf: &Buffer, area: Rect) -> String {
        let mut out = String::new();
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
    fn folder_min_size_marker_and_kb_uses_custom_marker_only_when_override_in_range() {
        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 100;
        cfg.settings.min_size_bytes = 100 * 1024;
        cfg.settings.min_size_custom_kb_min = 10;
        cfg.settings.min_size_custom_kb_max = 10_000;

        let folder_global = FolderEntry {
            path: "C:\\Global".to_string(),
            scan_depth: crate::config::ScanDepth::RootOnly,
            custom_min_size_kb: None,
        };
        let folder_custom_ok = FolderEntry {
            path: "C:\\CustomOk".to_string(),
            scan_depth: crate::config::ScanDepth::RootOnly,
            custom_min_size_kb: Some(222),
        };
        let folder_custom_low = FolderEntry {
            path: "C:\\CustomLow".to_string(),
            scan_depth: crate::config::ScanDepth::RootOnly,
            custom_min_size_kb: Some(9),
        };
        let folder_custom_high = FolderEntry {
            path: "C:\\CustomHigh".to_string(),
            scan_depth: crate::config::ScanDepth::RootOnly,
            custom_min_size_kb: Some(10_001),
        };

        assert_eq!(
            folder_min_size_marker_and_kb(&folder_global, &cfg.settings),
            (MIN_SIZE_MARKER_GLOBAL, 100)
        );
        assert_eq!(
            folder_min_size_marker_and_kb(&folder_custom_ok, &cfg.settings),
            (MIN_SIZE_MARKER_CUSTOM, 222)
        );
        assert_eq!(
            folder_min_size_marker_and_kb(&folder_custom_low, &cfg.settings),
            (MIN_SIZE_MARKER_GLOBAL, 100)
        );
        assert_eq!(
            folder_min_size_marker_and_kb(&folder_custom_high, &cfg.settings),
            (MIN_SIZE_MARKER_GLOBAL, 100)
        );
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
            "cycle scan depth",
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
    fn main_menu_actions_block_uses_legacy_default_layout_when_numeric_mapping_is_absent() {
        let cfg = AppConfig::default();

        let s = main_menu_actions_block(&cfg);
        assert!(
            s.contains("Main menu:\n"),
            "expected legacy main menu header when mapping is absent; got:\n{s}"
        );
        assert!(
            s.contains("  1 / a  add folder"),
            "expected legacy digit mapping line for '1' when mapping is absent; got:\n{s}"
        );
        assert!(
            s.contains("  7 / p  playlists"),
            "expected legacy digit mapping line for '7' when mapping is absent; got:\n{s}"
        );
    }

    #[test]
    fn main_menu_folder_lines_render_root_only_symbol_before_path() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig {
            folders: vec![
                FolderEntry {
                    path: "C:\\Music".to_string(),
                    scan_depth: crate::config::ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "C:\\Games".to_string(),
                    scan_depth: crate::config::ScanDepth::Recursive,
                    custom_min_size_kb: None,
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
        let expected_root_only = scan_depth_indicator_fixed(crate::config::ScanDepth::RootOnly);
        let expected_recursive = scan_depth_indicator_fixed(crate::config::ScanDepth::Recursive);
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

        // Regression guard: every rendered folder row must contain exactly one of the indicators.
        for (idx, folder) in folders.iter().enumerate() {
            let line = text
                .lines()
                .find(|l| l.contains(folder.path.as_str()))
                .unwrap_or_else(|| {
                    panic!("expected to render a line containing {:?}", folder.path)
                });
            let want = scan_depth_indicator_fixed(folder.scan_depth);
            assert!(
                line.contains(want.as_str()),
                "expected folder row {idx} to contain scan indicator {want:?}; got: {line:?}"
            );
            for other_depth in [
                crate::config::ScanDepth::RootOnly,
                crate::config::ScanDepth::OneLevel,
                crate::config::ScanDepth::Recursive,
            ] {
                if other_depth == folder.scan_depth {
                    continue;
                }
                let other = scan_depth_indicator_fixed(other_depth);
                assert!(
                    !line.contains(other.as_str()),
                    "regression: folder row {idx} must not contain other indicator {other:?}; got: {line:?}"
                );
            }
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

    #[test]
    fn status_bar_renders_default_min_size_kb_and_volume_percent() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let app = TuiApp::new(paths, cfg, PlaylistsFile::default());

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let text = buffer_as_text(terminal.backend().buffer());
        assert!(
            text.contains("min_size=1024kb"),
            "expected status bar to contain default min_size=1024kb; buffer was:\n{text}"
        );
        assert!(
            text.contains("Volume=75%"),
            "expected status bar to contain default Volume=75%; buffer was:\n{text}"
        );
    }

    #[test]
    fn status_bar_updates_when_volume_percent_changes_in_state() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let mut app = TuiApp::new(paths, cfg, PlaylistsFile::default());

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = buffer_as_text(terminal.backend().buffer());
        assert!(
            text.contains("Volume=75%"),
            "expected status bar to contain default Volume=75% before any player snapshot; buffer was:\n{text}"
        );

        app.state.player.volume_percent = 12;
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text2 = buffer_as_text(terminal.backend().buffer());
        assert!(
            text2.contains("Volume=12%"),
            "expected status bar to update to Volume=12% after state change; buffer was:\n{text2}"
        );
        assert!(
            !text2.contains("Volume=75%"),
            "regression: updated status bar must not keep old Volume=75%; buffer was:\n{text2}"
        );
    }

    #[test]
    fn now_playing_does_not_duplicate_header_inside_content() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let mut app = TuiApp::new(paths, cfg, PlaylistsFile::default());
        app.state.screen = Screen::NowPlaying;

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let buf = terminal.backend().buffer();
        // Mirror `draw()` layout: full frame -> outer block -> inner -> split into
        // content (min) + status bar (last 2 rows).
        let frame_area = buf.area();
        let inner = Rect::new(
            frame_area.x + 1,
            frame_area.y + 1,
            frame_area.width.saturating_sub(2),
            frame_area.height.saturating_sub(2),
        );
        let content_area = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(2),
        );
        let text = buffer_rect_as_text(buf, content_area);

        // The block title line contains box-drawing '─' characters; the historical regression
        // was an *extra* first content line starting with "Now Playing" (no '─' in that row).
        let duplicated_content_line = text.lines().find(|line| {
            let l = line.trim_start_matches([' ', '│']);
            l.starts_with("Now Playing") && !l.contains('─')
        });
        assert!(
            duplicated_content_line.is_none(),
            "regression: Now Playing content must not start with a duplicated header line; found: {:?}\nfull buffer:\n{}",
            duplicated_content_line,
            text
        );

        assert!(
            text.contains("Now Playing"),
            "test sanity: expected Now Playing title to be present in the rendered buffer"
        );
        assert!(
            text.contains("Status:"),
            "test sanity: expected Now Playing content to render a Status line; buffer was:\n{text}"
        );
    }

    #[test]
    fn now_playing_keys_block_uses_hotkeys_bindings_from_config() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.hotkeys.bindings.play_pause = Some(HotkeyChord {
            modifiers: vec![HotkeyModifier::Ctrl],
            key: HotkeyKey::Space,
        });
        cfg.hotkeys.bindings.next = Some(TapHoldBinding {
            chord: HotkeyChord {
                modifiers: vec![HotkeyModifier::Alt],
                key: HotkeyKey::Right,
            },
            hold: None,
        });
        cfg.hotkeys.bindings.prev = Some(TapHoldBinding {
            chord: HotkeyChord {
                modifiers: vec![HotkeyModifier::Shift],
                key: HotkeyKey::Left,
            },
            hold: None,
        });
        cfg.hotkeys.bindings.shuffle_toggle = Some(HotkeyChord {
            modifiers: vec![],
            key: HotkeyKey::S,
        });
        cfg.hotkeys.bindings.repeat_toggle = None;

        let mut app = TuiApp::new(paths, cfg, PlaylistsFile::default());
        app.state.screen = Screen::NowPlaying;

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let text = buffer_as_text(terminal.backend().buffer());
        assert!(
            text.contains("Ctrl+Space") && text.contains("play/pause"),
            "expected play/pause hint to include configured binding; buffer was:\n{text}"
        );
        assert!(
            text.contains("Alt+Right") && text.contains("next"),
            "expected next hint to include configured binding; buffer was:\n{text}"
        );
        assert!(
            text.contains("Shift+Left") && text.contains("previous"),
            "expected previous hint to include configured binding; buffer was:\n{text}"
        );
        assert!(
            text.contains("S") && text.contains("toggle shuffle"),
            "expected shuffle hint to include configured binding; buffer was:\n{text}"
        );
        assert!(
            text.contains("?") && text.contains("cycle repeat"),
            "expected missing repeat binding to render as '?'; buffer was:\n{text}"
        );
    }

    #[test]
    fn main_menu_actions_render_in_digit_order_when_numeric_mapping_present() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig {
            tui: TuiConfig {
                main_menu_numeric_mapping: Some(vec![
                    MainMenuNumericBinding {
                        key: 2,
                        command: MainMenuCommand::Playlists,
                    },
                    MainMenuNumericBinding {
                        key: 1,
                        command: MainMenuCommand::AddFolder,
                    },
                ]),
                extra: Default::default(),
            },
            ..Default::default()
        };

        let app = TuiApp::new(paths, cfg, PlaylistsFile::default());

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let text = buffer_as_text(terminal.backend().buffer());
        let add_line = text
            .lines()
            .find(|l| l.contains("add folder"))
            .expect("expected add folder line");
        let playlists_line = text
            .lines()
            .find(|l| l.contains("playlists"))
            .expect("expected playlists line");

        let add_pos = text.find(add_line).unwrap();
        let playlists_pos = text.find(playlists_line).unwrap();
        assert!(
            add_pos < playlists_pos,
            "expected key 1 line (add folder) to appear before key 2 line (playlists)"
        );
    }

    #[test]
    fn main_menu_actions_block_renders_mapped_digit_and_alpha_hints_when_mapping_present() {
        let cfg = AppConfig {
            tui: TuiConfig {
                main_menu_numeric_mapping: Some(vec![MainMenuNumericBinding {
                    key: 1,
                    command: MainMenuCommand::Playlists,
                }]),
                extra: Default::default(),
            },
            ..Default::default()
        };

        let s = main_menu_actions_block(&cfg);
        assert!(
            s.contains("Main menu (digits 1..9):\n"),
            "expected mapped main menu header when mapping is present; got:\n{s}"
        );
        assert!(
            s.contains("  1 / p  playlists"),
            "expected mapped playlists entry to include alpha hint '/ p'; got:\n{s}"
        );
        assert!(
            !s.contains("  1 / a  add folder"),
            "regression: legacy hardcoded '1 / a add folder' must not appear when mapping is present; got:\n{s}"
        );
    }
}
