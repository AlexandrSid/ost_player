use crate::config::MainMenuCommand;
use crate::error::AppResult;
use crate::tui::action::{Action, Screen};
use crate::tui::state::AppState;
use crate::tui::widgets::{ConfirmDialog, TextInput};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Default)]
pub struct MainMenuScreen {
    add_folder: Option<TextInput>,
    confirm_remove: Option<ConfirmDialog>,
    custom_min_size_input: Option<(usize, TextInput)>,
    confirm_quit: Option<ConfirmDialog>,
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

        if let Some(confirm) = &mut self.confirm_quit {
            if let Some(res) = confirm.on_key(key) {
                self.confirm_quit = None;
                return Ok(match res {
                    true => Some(Action::Quit),
                    false => Some(Action::ClearStatus),
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

        if let Some((idx, input)) = &mut self.custom_min_size_input {
            if let Some(done) = input.on_key(key) {
                let idx = *idx;
                self.custom_min_size_input = None;
                return Ok(match done {
                    crate::tui::widgets::TextInputResult::Submit(v) => {
                        let v = v.trim();
                        if v.is_empty() {
                            Some(Action::SetFolderCustomMinSizeKb {
                                idx,
                                custom_kb: None,
                            })
                        } else {
                            match v.parse::<u32>() {
                                Ok(n) => Some(Action::SetFolderCustomMinSizeKb {
                                    idx,
                                    custom_kb: Some(n),
                                }),
                                Err(_) => Some(Action::SetStatus(
                                    "custom min_size_kb must be an integer".to_string(),
                                )),
                            }
                        }
                    }
                    crate::tui::widgets::TextInputResult::Cancel => Some(Action::ClearStatus),
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
        if let Some((_idx, input)) = &mut self.custom_min_size_input {
            input.on_paste(text);
            return Ok(None);
        }
        Ok(None)
    }

    fn handle_normal_key(&mut self, state: &AppState, key: KeyEvent) -> Option<Action> {
        if let KeyCode::Char(ch) = key.code {
            if let Some(digit) = ch.to_digit(10) {
                let digit = digit as u8;
                if (1..=9).contains(&digit) {
                    if let Some(map) = state.cfg.tui.resolved_main_menu_numeric_mapping() {
                        if let Some(cmd) = map[(digit - 1) as usize] {
                            if cmd == MainMenuCommand::Play {
                                // Play is not part of the numeric main-menu mapping UX.
                                return None;
                            }
                            return self.dispatch_main_menu_command(state, cmd);
                        }
                        return None;
                    }
                }
            }
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if state.playlists_dirty {
                    self.confirm_quit = Some(ConfirmDialog::new(
                        "Unsaved playlist changes. Quit without saving?",
                        "Enter=yes  Esc=no",
                    ));
                    Some(Action::SetStatus("confirm quit...".to_string()))
                } else {
                    Some(Action::Quit)
                }
            }
            KeyCode::Up => Some(Action::SelectFolderDelta(-1)),
            KeyCode::Down => Some(Action::SelectFolderDelta(1)),
            KeyCode::Char('1') | KeyCode::Char('a') => {
                self.dispatch_main_menu_command(state, MainMenuCommand::AddFolder)
            }
            KeyCode::Char('2') | KeyCode::Char('d') => {
                self.dispatch_main_menu_command(state, MainMenuCommand::RemoveSelectedFolder)
            }
            KeyCode::Char('3') | KeyCode::Char('t') => self
                .dispatch_main_menu_command(state, MainMenuCommand::CycleSelectedFolderScanDepth),
            KeyCode::Char('4') | KeyCode::Char('c') => self.dispatch_main_menu_command(
                state,
                MainMenuCommand::SetSelectedFolderCustomMinSizeKb,
            ),
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.dispatch_main_menu_command(state, MainMenuCommand::Play)
            }
            KeyCode::Char('6') | KeyCode::Char('s') => Some(Action::Navigate(Screen::Settings)),
            KeyCode::Char('7') | KeyCode::Char('p') => Some(Action::Navigate(Screen::Playlists)),
            KeyCode::Char('8') | KeyCode::Char('r') => {
                self.dispatch_main_menu_command(state, MainMenuCommand::RescanLibrary)
            }
            _ => None,
        }
    }

    fn dispatch_main_menu_command(
        &mut self,
        state: &AppState,
        cmd: MainMenuCommand,
    ) -> Option<Action> {
        match cmd {
            MainMenuCommand::AddFolder => {
                self.add_folder = Some(TextInput::new(
                    "Add folder (absolute path)",
                    "",
                    "Enter=save  Esc=cancel",
                ));
                Some(Action::SetStatus("typing folder path...".to_string()))
            }
            MainMenuCommand::RemoveSelectedFolder => {
                if state.cfg.folders.is_empty() {
                    return Some(Action::SetStatus("no folders to remove".to_string()));
                }
                self.confirm_remove = Some(ConfirmDialog::new(
                    "Remove selected folder?",
                    "Enter=yes  Esc=no",
                ));
                Some(Action::SetStatus("confirm removal...".to_string()))
            }
            MainMenuCommand::CycleSelectedFolderScanDepth => {
                if state.cfg.folders.is_empty() {
                    Some(Action::SetStatus("no folders to toggle".to_string()))
                } else {
                    Some(Action::ToggleFolderRootOnlyAt(state.main_selected_folder))
                }
            }
            MainMenuCommand::SetSelectedFolderCustomMinSizeKb => {
                if state.cfg.folders.is_empty() {
                    return Some(Action::SetStatus("no folders to edit".to_string()));
                }
                let idx = state
                    .main_selected_folder
                    .min(state.cfg.folders.len().saturating_sub(1));
                let initial = state
                    .cfg
                    .folders
                    .get(idx)
                    .and_then(|f| f.custom_min_size_kb)
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let min = state.cfg.settings.min_size_custom_kb_min;
                let max = state.cfg.settings.min_size_custom_kb_max;
                self.custom_min_size_input = Some((
                    idx,
                    TextInput::new(
                        "Set custom min_size_kb for selected folder (empty = clear)",
                        initial.as_str(),
                        &format!("Range {min}..={max}  Enter=save  Esc=cancel"),
                    ),
                ));
                Some(Action::SetStatus(
                    "editing custom min_size_kb...".to_string(),
                ))
            }
            MainMenuCommand::Play => Some(Action::PlayerLoadFromLibrary { start_index: 0 }),
            MainMenuCommand::Settings => Some(Action::Navigate(Screen::Settings)),
            MainMenuCommand::Playlists => Some(Action::Navigate(Screen::Playlists)),
            MainMenuCommand::RescanLibrary => {
                if state.cfg.folders.is_empty() {
                    Some(Action::SetStatus(
                        "no folders configured to scan".to_string(),
                    ))
                } else {
                    Some(Action::RescanLibrary)
                }
            }
        }
    }

    pub fn view<'a>(&'a self, state: &'a AppState) -> MainMenuView<'a> {
        MainMenuView {
            add_folder: self.add_folder.as_ref(),
            confirm_remove: self.confirm_remove.as_ref(),
            confirm_quit: self.confirm_quit.as_ref(),
            custom_min_size_input: self.custom_min_size_input.as_ref().map(|(_, i)| i),
            folders: &state.cfg.folders,
            selected_folder: state.main_selected_folder,
        }
    }
}

pub struct MainMenuView<'a> {
    pub add_folder: Option<&'a TextInput>,
    pub confirm_remove: Option<&'a ConfirmDialog>,
    pub confirm_quit: Option<&'a ConfirmDialog>,
    pub custom_min_size_input: Option<&'a TextInput>,
    pub folders: &'a [crate::config::FolderEntry],
    pub selected_folder: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, FolderEntry};
    use crate::config::{MainMenuCommand, MainMenuNumericBinding, TuiConfig};
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
            config_path: data_dir.join("config.yaml"),
            playlists_path: data_dir.join("playlists.yaml"),
            state_path: data_dir.join("state.yaml"),
        }
    }

    fn make_state(base_dir: &Path, folders: Vec<FolderEntry>) -> AppState {
        let paths = paths_for(base_dir);
        let cfg = AppConfig {
            folders,
            ..Default::default()
        };
        AppState::new(
            paths,
            cfg,
            PlaylistsFile::default(),
            LibraryIndex::default(),
        )
    }

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty())
    }

    fn key_code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn key_t_emits_toggle_action_for_selected_folder_index() {
        let td = tempfile::tempdir().unwrap();
        let mut state = make_state(
            td.path(),
            vec![
                FolderEntry::new("A".to_string()),
                FolderEntry::new("B".to_string()),
            ],
        );
        state.main_selected_folder = 1;

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('t')).unwrap();

        assert_eq!(action, Some(Action::ToggleFolderRootOnlyAt(1)));
    }

    #[test]
    fn key_3_emits_toggle_action_for_selected_folder_index() {
        let td = tempfile::tempdir().unwrap();
        let mut state = make_state(
            td.path(),
            vec![
                FolderEntry::new("A".to_string()),
                FolderEntry::new("B".to_string()),
            ],
        );
        state.main_selected_folder = 1;

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('3')).unwrap();

        assert_eq!(action, Some(Action::ToggleFolderRootOnlyAt(1)));
    }

    #[test]
    fn key_4_with_no_folders_emits_status_error_for_custom_min_size_edit() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('4')).unwrap();

        assert!(matches!(action, Some(Action::SetStatus(_))));
    }

    #[test]
    fn key_4_with_folders_opens_custom_min_size_modal_and_sets_status() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![FolderEntry::new("A".to_string())]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('4')).unwrap();

        assert_eq!(
            action,
            Some(Action::SetStatus(
                "editing custom min_size_kb...".to_string()
            ))
        );
        assert!(screen.view(&state).custom_min_size_input.is_some());
    }

    #[test]
    fn key_enter_emits_play_action() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen
            .on_key(&state, KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();

        assert_eq!(
            action,
            Some(Action::PlayerLoadFromLibrary { start_index: 0 })
        );
    }

    #[test]
    fn key_space_emits_play_action() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen
            .on_key(
                &state,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
            )
            .unwrap();

        assert_eq!(
            action,
            Some(Action::PlayerLoadFromLibrary { start_index: 0 })
        );
    }

    #[test]
    fn key_up_and_down_select_folder_delta() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let inputs = [
            (
                KeyEvent::new(KeyCode::Up, KeyModifiers::empty()),
                Some(Action::SelectFolderDelta(-1)),
            ),
            (
                KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
                Some(Action::SelectFolderDelta(1)),
            ),
        ];
        for (k, expected) in inputs {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert_eq!(action, expected);
        }
    }

    #[test]
    fn key_1_and_a_start_add_folder_text_input() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        for k in [key('1'), key('a')] {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert!(matches!(action, Some(Action::SetStatus(_))));

            let v = screen.view(&state);
            assert!(v.add_folder.is_some());
            assert!(v.confirm_remove.is_none());
        }
    }

    #[test]
    fn when_numeric_mapping_is_absent_digit_1_keeps_default_behavior_add_folder() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('1')).unwrap();
        assert!(
            matches!(action, Some(Action::SetStatus(_))),
            "expected default main menu digit '1' to start add-folder flow when mapping is absent"
        );
        assert!(screen.view(&state).add_folder.is_some());
    }

    #[test]
    fn esc_in_add_folder_modal_cancels_input_and_does_not_quit() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('1')).unwrap();
        assert!(matches!(action, Some(Action::SetStatus(_))));
        assert!(screen.view(&state).add_folder.is_some());

        let action = screen
            .on_key(&state, KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()))
            .unwrap();
        assert_eq!(action, Some(Action::ClearStatus));
        assert!(screen.view(&state).add_folder.is_none());
    }

    #[test]
    fn typing_in_add_folder_modal_updates_text_input_buffer() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('1')).unwrap();
        assert!(matches!(action, Some(Action::SetStatus(_))));

        // Type a typical Windows absolute path.
        for ch in ['C', ':', '\\', 'G', 'a', 'm', 'e', 's'] {
            let action = screen.on_key(&state, key(ch)).unwrap();
            assert_eq!(action, None);
        }

        let v = screen.view(&state);
        let input = v.add_folder.expect("expected add_folder modal to be open");
        assert_eq!(input.value, r"C:\Games");
    }

    #[test]
    fn enter_in_add_folder_modal_submits_action_and_closes_modal() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let _ = screen.on_key(&state, key('1')).unwrap();
        for ch in ['C', ':', '\\', 'M', 'u', 's', 'i', 'c'] {
            let _ = screen.on_key(&state, key(ch)).unwrap();
        }

        let action = screen.on_key(&state, key_code(KeyCode::Enter)).unwrap();
        assert_eq!(action, Some(Action::AddFolder(r"C:\Music".to_string())));
        assert!(screen.view(&state).add_folder.is_none());
    }

    #[test]
    fn reopen_add_folder_modal_starts_with_empty_input() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let _ = screen.on_key(&state, key('1')).unwrap();
        let _ = screen.on_key(&state, key('X')).unwrap();

        // Cancel the first modal.
        let action = screen.on_key(&state, key_code(KeyCode::Esc)).unwrap();
        assert_eq!(action, Some(Action::ClearStatus));
        assert!(screen.view(&state).add_folder.is_none());

        // Open again; it must start empty.
        let _ = screen.on_key(&state, key('1')).unwrap();
        let v = screen.view(&state);
        let input = v.add_folder.expect("expected add_folder modal to be open");
        assert_eq!(input.value, "");
    }

    #[test]
    fn key_a_can_open_and_submit_add_folder_modal() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('a')).unwrap();
        assert!(matches!(action, Some(Action::SetStatus(_))));
        assert!(screen.view(&state).add_folder.is_some());

        for ch in ['C', ':', '\\', 'M', 'u', 's', 'i', 'c'] {
            let _ = screen.on_key(&state, key(ch)).unwrap();
        }
        let action = screen.on_key(&state, key_code(KeyCode::Enter)).unwrap();
        assert_eq!(action, Some(Action::AddFolder(r"C:\Music".to_string())));
        assert!(screen.view(&state).add_folder.is_none());
    }

    #[test]
    fn cancel_add_folder_modal_does_not_dispatch_add_folder() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let _ = screen.on_key(&state, key('1')).unwrap();
        for ch in ['C', ':', '\\', 'M', 'u', 's', 'i', 'c'] {
            let _ = screen.on_key(&state, key(ch)).unwrap();
        }

        // Cancel: should clear status and close modal (no AddFolder dispatch).
        let action = screen.on_key(&state, key_code(KeyCode::Esc)).unwrap();
        assert_eq!(action, Some(Action::ClearStatus));
        assert!(screen.view(&state).add_folder.is_none());

        // After cancel, Enter is handled by the main menu (play), not AddFolder.
        let action = screen.on_key(&state, key_code(KeyCode::Enter)).unwrap();
        assert_eq!(
            action,
            Some(Action::PlayerLoadFromLibrary { start_index: 0 })
        );
    }

    #[test]
    fn on_paste_routes_into_add_folder_input_only_when_modal_is_open() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();

        // When modal is closed, paste should not open it or modify state.
        let action = screen.on_paste(&state, r"C:\Games").unwrap();
        assert_eq!(action, None);
        assert!(screen.view(&state).add_folder.is_none());

        // Open the add-folder modal and paste should populate its buffer.
        let _ = screen.on_key(&state, key('1')).unwrap();
        assert!(screen.view(&state).add_folder.is_some());

        let action = screen.on_paste(&state, r"C:\Games").unwrap();
        assert_eq!(action, None);

        let v = screen.view(&state);
        let input = v.add_folder.expect("expected add_folder modal to be open");
        assert_eq!(input.value, r"C:\Games");
    }

    #[test]
    fn key_2_and_d_open_confirm_remove_when_folders_exist() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![FolderEntry::new("A".to_string())]);

        for k in [key('2'), key('d')] {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert!(matches!(action, Some(Action::SetStatus(_))));

            let v = screen.view(&state);
            assert!(v.add_folder.is_none());
            assert!(v.confirm_remove.is_some());
        }
    }

    #[test]
    fn esc_in_confirm_remove_modal_cancels_and_does_not_quit() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![FolderEntry::new("A".to_string())]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('2')).unwrap();
        assert!(matches!(action, Some(Action::SetStatus(_))));
        assert!(screen.view(&state).confirm_remove.is_some());

        let action = screen
            .on_key(&state, KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()))
            .unwrap();
        assert_eq!(action, Some(Action::ClearStatus));
        assert!(screen.view(&state).confirm_remove.is_none());
    }

    #[test]
    fn key_2_and_d_when_no_folders_sets_status_instead_of_opening_confirm() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        for k in [key('2'), key('d')] {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert!(matches!(action, Some(Action::SetStatus(_))));

            let v = screen.view(&state);
            assert!(v.add_folder.is_none());
            assert!(v.confirm_remove.is_none());
        }
    }

    #[test]
    fn key_6_and_s_navigate_to_settings() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        for k in [key('6'), key('s')] {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert_eq!(action, Some(Action::Navigate(Screen::Settings)));
        }
    }

    #[test]
    fn key_6_and_p_navigate_to_playlists() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        for k in [key('7'), key('p')] {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert_eq!(action, Some(Action::Navigate(Screen::Playlists)));
        }
    }

    #[test]
    fn key_7_and_r_rescans_when_folders_exist() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![FolderEntry::new("A".to_string())]);

        for k in [key('8'), key('r')] {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert_eq!(action, Some(Action::RescanLibrary));
        }
    }

    #[test]
    fn key_7_and_r_when_no_folders_sets_status_instead_of_rescanning() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        for k in [key('8'), key('r')] {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert!(matches!(action, Some(Action::SetStatus(_))));
        }
    }

    #[test]
    fn key_q_and_esc_quit_when_not_dirty() {
        let td = tempfile::tempdir().unwrap();
        let mut state = make_state(td.path(), vec![]);
        state.playlists_dirty = false;

        let inputs = [key('q'), KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())];
        for k in inputs {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert_eq!(action, Some(Action::Quit));
        }
    }

    #[test]
    fn key_q_and_esc_when_dirty_opens_confirm_quit_modal_instead_of_quitting() {
        let td = tempfile::tempdir().unwrap();
        let mut state = make_state(td.path(), vec![]);
        state.playlists_dirty = true;

        let inputs = [key('q'), KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())];
        for k in inputs {
            let mut screen = MainMenuScreen::default();
            let action = screen.on_key(&state, k).unwrap();
            assert_eq!(
                action,
                Some(Action::SetStatus("confirm quit...".to_string()))
            );
            assert!(
                screen.view(&state).confirm_quit.is_some(),
                "expected confirm quit modal to be open when dirty"
            );
        }
    }

    #[test]
    fn confirm_quit_modal_enter_quits_and_esc_cancels() {
        let td = tempfile::tempdir().unwrap();
        let mut state = make_state(td.path(), vec![]);
        state.playlists_dirty = true;

        // Open confirm quit modal.
        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('q')).unwrap();
        assert_eq!(
            action,
            Some(Action::SetStatus("confirm quit...".to_string()))
        );
        assert!(screen.view(&state).confirm_quit.is_some());

        // Esc cancels and clears status.
        let action = screen
            .on_key(&state, KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()))
            .unwrap();
        assert_eq!(action, Some(Action::ClearStatus));
        assert!(screen.view(&state).confirm_quit.is_none());

        // Open again; Enter confirms quit.
        let _ = screen.on_key(&state, key('q')).unwrap();
        let action = screen
            .on_key(&state, KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
            .unwrap();
        assert_eq!(action, Some(Action::Quit));
        assert!(screen.view(&state).confirm_quit.is_none());
    }

    #[test]
    fn key_5_does_not_trigger_play_or_exit() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('5')).unwrap();
        assert_eq!(
            action, None,
            "UX-001 regression guard: '5' must not trigger play (Enter/Space) or exit (Esc/q)"
        );
    }

    #[test]
    fn key_0_does_not_trigger_play_or_exit() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('0')).unwrap();
        assert_eq!(
            action, None,
            "UX-001 regression guard: '0' must not trigger exit (Esc/q) or play (Enter/Space)"
        );
    }

    #[test]
    fn key_t_when_no_folders_sets_status_instead_of_toggling() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('t')).unwrap();

        assert!(matches!(action, Some(Action::SetStatus(_))));
    }

    #[test]
    fn key_3_when_no_folders_sets_status_instead_of_toggling() {
        let td = tempfile::tempdir().unwrap();
        let state = make_state(td.path(), vec![]);

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('3')).unwrap();

        assert!(matches!(action, Some(Action::SetStatus(_))));
    }

    #[test]
    fn numeric_mapping_overrides_digit_1_to_dispatch_mapped_command() {
        let td = tempfile::tempdir().unwrap();
        let mut state = make_state(td.path(), vec![]);
        state.cfg.tui = TuiConfig {
            main_menu_numeric_mapping: Some(vec![MainMenuNumericBinding {
                key: 1,
                command: MainMenuCommand::Playlists,
            }]),
            extra: Default::default(),
        };

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('1')).unwrap();

        assert_eq!(action, Some(Action::Navigate(Screen::Playlists)));
    }

    #[test]
    fn numeric_mapping_with_gaps_does_not_fallback_to_default_digit_behavior() {
        let td = tempfile::tempdir().unwrap();
        let mut state = make_state(td.path(), vec![]);
        state.cfg.tui = TuiConfig {
            // Only bind key 1. Key 2 is intentionally absent.
            main_menu_numeric_mapping: Some(vec![MainMenuNumericBinding {
                key: 1,
                command: MainMenuCommand::Playlists,
            }]),
            extra: Default::default(),
        };

        let mut screen = MainMenuScreen::default();
        let action = screen.on_key(&state, key('2')).unwrap();

        assert_eq!(
            action, None,
            "when numeric mapping is present but a digit is not mapped, it should be ignored (no fallback)"
        );
    }
}
