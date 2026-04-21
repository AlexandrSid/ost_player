use crate::config::{self, effective_min_size_kb_for_folder, FolderEntry, RepeatMode, ScanDepth};
use crate::error::{AppError, AppResult};
use crate::indexer::{self, FolderScanEntry, LibraryIndex, ScanOptions};
use crate::player::{PlayerCommand, PlayerEvent, PlayerHandle};
use crate::playlists::{self, Playlist};
use crate::state as app_state_file;
use crate::tui::action::{Action, Screen};
use crate::tui::screens::{MainMenuScreen, NowPlayingScreen, PlaylistsScreen, SettingsScreen};
use crate::tui::state::{AppState, PlaybackSource};
use crate::{config::AppConfig, paths::AppPaths, playlists::PlaylistsFile};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub trait Persistence {
    fn save_config(&self, paths: &AppPaths, cfg: &AppConfig) -> AppResult<()>;
    fn save_playlists(&self, paths: &AppPaths, pls: &PlaylistsFile) -> AppResult<()>;
}

#[derive(Default)]
struct RealPersistence;

impl Persistence for RealPersistence {
    fn save_config(&self, paths: &AppPaths, cfg: &AppConfig) -> AppResult<()> {
        config::io::save(paths, cfg)
    }

    fn save_playlists(&self, paths: &AppPaths, pls: &PlaylistsFile) -> AppResult<()> {
        playlists::io::save(paths, pls)
    }
}

pub struct TuiApp {
    pub state: AppState,
    pub(crate) main_menu: MainMenuScreen,
    pub(crate) settings: SettingsScreen,
    pub(crate) playlists: PlaylistsScreen,
    pub(crate) now_playing: NowPlayingScreen,
    persistence: Box<dyn Persistence>,
    player: Option<PlayerHandle>,
    rng: StdRng,
    scan_spawner: Box<dyn ScanSpawner>,
    scan_job: Option<ScanJobState>,
    pending_scan: Option<ScanRequest>,
    debounced_scan: Option<DebouncedScan>,
    latest_bg_scan_token: u64,
    pending_play_from_library: Option<usize>,
    #[cfg(test)]
    load_queue_commands_sent: usize,
}

fn library_tracks_to_paths(library: &crate::indexer::LibraryIndex) -> Vec<std::path::PathBuf> {
    library.tracks.iter().map(|t| t.path.clone()).collect()
}

fn find_track_index_by_path(tracks: &[PathBuf], needle: &PathBuf) -> Option<usize> {
    // Fast path: byte-for-byte `PathBuf` equality.
    if let Some(idx) = tracks.iter().position(|p| p == needle) {
        return Some(idx);
    }

    // Slow path: compare canonicalized paths. This helps on Windows when one side includes a
    // verbatim prefix (\\?\) or different normalization, but still points to the same file.
    let needle_can = std::fs::canonicalize(needle).ok()?;
    tracks.iter().position(|p| {
        if p == needle {
            return true;
        }
        std::fs::canonicalize(p).ok().as_ref() == Some(&needle_can)
    })
}

fn clamp_start_index(start_index: usize, tracks_len: usize) -> usize {
    if tracks_len == 0 {
        0
    } else {
        start_index.min(tracks_len.saturating_sub(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanOrigin {
    RescanLibrary,
    PlayFromLibrary { start_index: usize },
    LoadPlaylist { stopped_playback: bool },
    BackgroundRescan { token: u64 },
}

#[derive(Debug)]
struct ScanRequest {
    folders: Vec<FolderScanEntry>,
    opts: ScanOptions,
    origin: ScanOrigin,
}

#[derive(Debug)]
struct ScanJobState {
    origin: ScanOrigin,
    #[cfg(test)]
    started_at: Instant,
    rx: mpsc::Receiver<crate::indexer::LibraryIndex>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct DebouncedScan {
    due_at: Instant,
    req: ScanRequest,
}

trait ScanSpawner {
    fn spawn_scan(&self, folders: Vec<FolderScanEntry>, opts: ScanOptions) -> ScanJobState;
}

#[derive(Default)]
struct ThreadedScanSpawner;

impl ScanSpawner for ThreadedScanSpawner {
    fn spawn_scan(&self, folders: Vec<FolderScanEntry>, opts: ScanOptions) -> ScanJobState {
        let (tx, rx) = mpsc::channel::<crate::indexer::LibraryIndex>();
        let origin_placeholder = ScanOrigin::RescanLibrary; // overwritten by caller
        #[cfg(test)]
        let started_at = Instant::now();
        let handle = std::thread::spawn(move || {
            let index = indexer::scan::scan_library_folders(&folders, &opts);
            let _ = tx.send(index);
        });
        ScanJobState {
            origin: origin_placeholder,
            #[cfg(test)]
            started_at,
            rx,
            handle: Some(handle),
        }
    }
}

impl TuiApp {
    pub fn new(
        paths: crate::paths::AppPaths,
        cfg: crate::config::AppConfig,
        pls: crate::playlists::PlaylistsFile,
    ) -> Self {
        Self::new_with_deps(
            paths,
            cfg,
            pls,
            Box::new(RealPersistence),
            Box::new(ThreadedScanSpawner),
        )
    }

    fn new_with_deps(
        paths: crate::paths::AppPaths,
        cfg: crate::config::AppConfig,
        pls: crate::playlists::PlaylistsFile,
        persistence: Box<dyn Persistence>,
        scan_spawner: Box<dyn ScanSpawner>,
    ) -> Self {
        let cached = indexer::io::load_best_effort(&paths).unwrap_or_default();
        let player = PlayerHandle::spawn(
            cfg.settings.shuffle,
            cfg.settings.repeat,
            cfg.audio.volume_default_percent,
        );
        let rng = StdRng::from_entropy();
        let mut state = AppState::new(paths, cfg, pls, cached);
        state.player.shuffle = state.cfg.settings.shuffle;
        state.player.repeat = state.cfg.settings.repeat;
        Self {
            state,
            main_menu: MainMenuScreen::default(),
            settings: SettingsScreen::default(),
            playlists: PlaylistsScreen::default(),
            now_playing: NowPlayingScreen,
            persistence,
            player: Some(player),
            rng,
            scan_spawner,
            scan_job: None,
            pending_scan: None,
            debounced_scan: None,
            latest_bg_scan_token: 0,
            pending_play_from_library: None,
            #[cfg(test)]
            load_queue_commands_sent: 0,
        }
    }

    fn random_start_index(&mut self, tracks_len: usize) -> usize {
        if tracks_len == 0 {
            0
        } else {
            self.rng.gen_range(0..tracks_len)
        }
    }

    #[cfg(test)]
    fn set_rng_seed_for_test(&mut self, seed: u64) {
        self.rng = StdRng::seed_from_u64(seed);
    }

    pub fn tick(&mut self) -> AppResult<Option<Action>> {
        self.drain_player_events();
        self.poll_scan_job_completion();
        self.maybe_start_debounced_scan();
        match self.state.screen {
            Screen::MainMenu => self.main_menu.on_tick(&self.state),
            Screen::Settings => self.settings.on_tick(&self.state),
            Screen::Playlists => self.playlists.on_tick(&self.state),
            Screen::NowPlaying => self.now_playing.on_tick(&self.state),
            Screen::Folders => Ok(Some(Action::SetStatus("not implemented yet".to_string()))),
        }
    }

    pub fn on_key(&mut self, key: crossterm::event::KeyEvent) -> AppResult<Option<Action>> {
        self.drain_player_events();
        match self.state.screen {
            Screen::MainMenu => self.main_menu.on_key(&self.state, key),
            Screen::Settings => self.settings.on_key(&self.state, key),
            Screen::Playlists => self.playlists.on_key(&self.state, key),
            Screen::NowPlaying => self.now_playing.on_key(&self.state, key),
            Screen::Folders => Ok(None),
        }
    }

    pub fn on_paste(&mut self, text: &str) -> AppResult<Option<Action>> {
        self.drain_player_events();
        match self.state.screen {
            Screen::MainMenu => self.main_menu.on_paste(&self.state, text),
            Screen::Settings => self.settings.on_paste(&self.state, text),
            Screen::Playlists => self.playlists.on_paste(&self.state, text),
            Screen::NowPlaying => self.now_playing.on_paste(&self.state, text),
            Screen::Folders => Ok(None),
        }
    }

    fn schedule_debounced_background_rescan(&mut self) -> AppResult<()> {
        if self.state.cfg.folders.is_empty() {
            return Ok(());
        }
        self.latest_bg_scan_token = self.latest_bg_scan_token.wrapping_add(1);
        let token = self.latest_bg_scan_token;
        let req = self.active_folders_scan_request(ScanOrigin::BackgroundRescan { token })?;

        // Debounce to absorb bursts of folder edits.
        let due_at = Instant::now() + Duration::from_millis(200);
        self.debounced_scan = Some(DebouncedScan { due_at, req });
        Ok(())
    }

    fn maybe_start_debounced_scan(&mut self) {
        let Some(pending) = self.debounced_scan.as_ref() else {
            return;
        };
        if Instant::now() < pending.due_at {
            return;
        }
        let DebouncedScan { req, .. } = self.debounced_scan.take().expect("checked above");
        self.request_scan(req);
    }

    pub fn apply(&mut self, action: Action) -> AppResult<()> {
        match action {
            Action::Quit => {
                // Terminal loop handles quitting.
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::Shutdown);
                }
                // Best-effort cleanup for background scans: join if already finished.
                if let Some(job) = self.scan_job.as_mut() {
                    if let Some(h) = job.handle.take() {
                        if h.is_finished() {
                            let _ = h.join();
                        } else {
                            job.handle = Some(h);
                        }
                    }
                }
            }
            Action::Navigate(screen) => {
                self.state.screen = screen;
                self.state.status = None;
            }
            Action::SetStatus(msg) => self.state.status = Some(msg),
            Action::ClearStatus => self.state.status = None,
            Action::PlayerSetUiActivity { focused, minimized } => {
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::SetUiActivity { focused, minimized });
                }
            }
            Action::SelectFolderDelta(delta) => {
                let len = self.state.cfg.folders.len();
                if len == 0 {
                    self.state.main_selected_folder = 0;
                    return Ok(());
                }
                let cur = self.state.main_selected_folder as i32;
                let next = (cur + delta).clamp(0, (len - 1) as i32) as usize;
                self.state.main_selected_folder = next;
            }
            Action::SelectPlaylistDelta(delta) => {
                let len = self.state.playlists.playlists.len();
                if len == 0 {
                    self.state.playlists_selected = 0;
                    return Ok(());
                }
                let cur = self.state.playlists_selected as i32;
                let next = (cur + delta).clamp(0, (len - 1) as i32) as usize;
                self.state.playlists_selected = next;
            }

            Action::AddFolder(folder) => {
                let trimmed = folder.trim();
                if trimmed.is_empty() {
                    self.state.status = Some("folder path must not be empty".to_string());
                    return Ok(());
                }
                self.state
                    .cfg
                    .folders
                    .push(FolderEntry::new(trimmed.to_string()));
                self.state.cfg = self.state.cfg.clone().normalized();
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                tracing::info!(target: crate::logging::PERSIST_LOG_TARGET, path = %trimmed, "folder added");
                self.state.status = Some("folder added and saved".to_string());
                self.schedule_debounced_background_rescan()?;
            }
            Action::RemoveFolderAt(idx) => {
                if idx >= self.state.cfg.folders.len() {
                    return Ok(());
                }
                let removed = self.state.cfg.folders[idx].path.clone();
                self.state.cfg.folders.remove(idx);
                self.state.cfg = self.state.cfg.clone().normalized();
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                if self.state.main_selected_folder >= self.state.cfg.folders.len() {
                    self.state.main_selected_folder =
                        self.state.cfg.folders.len().saturating_sub(1);
                }
                tracing::info!(target: crate::logging::PERSIST_LOG_TARGET, path = %removed, "folder removed");
                self.state.status = Some("folder removed and saved".to_string());
                self.schedule_debounced_background_rescan()?;
            }
            Action::ToggleFolderRootOnlyAt(idx) => {
                let (path, new_depth) = {
                    let Some(folder) = self.state.cfg.folders.get_mut(idx) else {
                        return Ok(());
                    };
                    let new_depth = folder.scan_depth.cycle_next();
                    folder.scan_depth = new_depth;
                    (folder.path.clone(), new_depth)
                };
                self.state.cfg = self.state.cfg.clone().normalized();
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                let label = match new_depth {
                    ScanDepth::RootOnly => "root-only",
                    ScanDepth::OneLevel => "one-level",
                    ScanDepth::Recursive => "recursive",
                };
                tracing::info!(
                    target: crate::logging::PERSIST_LOG_TARGET,
                    path = %path,
                    scan_depth = ?new_depth,
                    "folder setting changed"
                );
                self.state.status = Some(format!("scan depth: {label}"));
                self.schedule_debounced_background_rescan()?;
            }
            Action::SetFolderCustomMinSizeKb { idx, custom_kb } => {
                let (min, max) = (
                    self.state.cfg.settings.min_size_custom_kb_min,
                    self.state.cfg.settings.min_size_custom_kb_max,
                );

                let (applied, path, next_val) = {
                    let Some(folder) = self.state.cfg.folders.get_mut(idx) else {
                        return Ok(());
                    };
                    let applied = match custom_kb {
                        None => {
                            folder.custom_min_size_kb = None;
                            true
                        }
                        Some(v) => {
                            if (min..=max).contains(&v) {
                                folder.custom_min_size_kb = Some(v);
                                true
                            } else {
                                // Ignore out-of-range values (keep existing value unchanged).
                                false
                            }
                        }
                    };
                    (applied, folder.path.clone(), folder.custom_min_size_kb)
                };

                if !applied {
                    self.state.status = Some(format!(
                        "ignored: custom min_size_kb must be within {min}..={max}"
                    ));
                    return Ok(());
                }

                self.state.cfg = self.state.cfg.clone().normalized();
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                tracing::info!(
                    target: crate::logging::PERSIST_LOG_TARGET,
                    path = %path,
                    custom_min_size_kb = ?next_val,
                    "folder setting changed"
                );
                self.state.status = Some("settings saved".to_string());
                self.schedule_debounced_background_rescan()?;
            }
            Action::SetMinSizeKb(v) => {
                let bytes = match v.checked_mul(1024) {
                    Some(b) => b,
                    None => {
                        self.state.status = Some("settings.min_size_kb is too large".to_string());
                        return Ok(());
                    }
                };
                self.state.cfg.settings.min_size_kb = v;
                self.state.cfg.settings.min_size_bytes = bytes;
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                tracing::info!(target: crate::logging::PERSIST_LOG_TARGET, min_size_kb = v, "setting changed");
                self.state.status = Some("settings saved".to_string());
                self.schedule_debounced_background_rescan()?;
            }
            Action::ToggleShuffle => {
                self.state.cfg.settings.shuffle = !self.state.cfg.settings.shuffle;
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                tracing::info!(
                    target: crate::logging::PERSIST_LOG_TARGET,
                    shuffle = self.state.cfg.settings.shuffle,
                    "setting changed"
                );
                self.state.status = Some("settings saved".to_string());
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::SetShuffle(self.state.cfg.settings.shuffle));
                }
            }
            Action::CycleRepeat => {
                self.state.cfg.settings.repeat = match self.state.cfg.settings.repeat {
                    RepeatMode::Off => RepeatMode::All,
                    RepeatMode::All => RepeatMode::One,
                    RepeatMode::One => RepeatMode::Off,
                };
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                tracing::info!(
                    target: crate::logging::PERSIST_LOG_TARGET,
                    repeat = ?self.state.cfg.settings.repeat,
                    "setting changed"
                );
                self.state.status = Some("settings saved".to_string());
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::SetRepeat(self.state.cfg.settings.repeat));
                }
            }

            Action::PlayerLoadFromLibrary { start_index } => {
                if self.state.cfg.folders.is_empty() {
                    self.state.status = Some("no folders configured to scan".to_string());
                    return Ok(());
                }

                // NP-001: Guard: if the requested source matches the currently playing/paused
                // source, only navigate to Now Playing (no scan, no queue reload).
                let is_playing_or_paused =
                    self.state.player.status != crate::player::PlaybackStatus::Stopped;
                let requested_source = PlaybackSource::from_active_playlist_or_folders(
                    self.state.playlists.active.as_deref(),
                    &self.state.cfg.folders,
                );
                if is_playing_or_paused
                    && self.state.playback_source.as_ref() == Some(&requested_source)
                {
                    self.state.screen = Screen::NowPlaying;
                    self.state.status = None;
                    return Ok(());
                }

                // FIX-004: Always rescan ACTIVE folders before building the queue, to avoid
                // enqueueing stale tracks after folder changes.
                let desired_fp = crate::indexer::compute_index_fingerprint(
                    &self
                        .state
                        .cfg
                        .folders
                        .iter()
                        .map(|f| {
                            let eff_kb =
                                effective_min_size_kb_for_folder(f, &self.state.cfg.settings);
                            let min_size_bytes = eff_kb.saturating_mul(1024);
                            FolderScanEntry {
                                path: f.path.clone(),
                                scan_depth: f.scan_depth,
                                min_size_bytes,
                            }
                        })
                        .collect::<Vec<_>>(),
                    &ScanOptions {
                        supported_extensions: self.state.cfg.settings.supported_extensions.clone(),
                        min_size_bytes: self.state.cfg.settings.min_size_kb.saturating_mul(1024),
                        allow_name_size_fallback_dedup: true,
                        force_canonicalize_fail: false,
                    },
                );
                if self.scan_job.is_none()
                    && !self.state.library.tracks.is_empty()
                    && self.state.library.schema_version == LibraryIndex::SCHEMA_VERSION
                    && self.state.library.index_fingerprint == desired_fp
                {
                    // Index inputs match; reuse immediately without blocking on a scan.
                    self.load_queue_from_current_library(start_index);
                    return Ok(());
                }
                if self.scan_job.is_some() {
                    // Do not start a second scan; mark play pending and complete it right after
                    // the current scan finishes.
                    self.state.screen = Screen::NowPlaying;
                    self.pending_play_from_library = Some(start_index);
                    self.state.status = Some("Scanning... (play pending)".to_string());
                    return Ok(());
                }

                // TZ-003: Optimistically navigate immediately. The queue will be loaded when the
                // scan completes.
                self.state.screen = Screen::NowPlaying;

                let req =
                    self.active_folders_scan_request(ScanOrigin::PlayFromLibrary { start_index })?;
                self.request_scan(req);
                // If the scan spawner completes synchronously (tests), apply immediately.
                self.poll_scan_job_completion();
            }
            Action::PlayerTogglePlayPause => {
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::TogglePlayPause);
                }
            }
            Action::PlayerStop => {
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::Stop);
                }
            }
            Action::PlayerNext => {
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::Next);
                }
            }
            Action::PlayerPrev => {
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::Prev);
                }
            }
            Action::PlayerSeekRelativeSeconds(delta) => {
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::SeekRelativeSeconds(delta));
                }
            }
            Action::VolumeUp => {
                if let Some(p) = self.player.as_ref() {
                    let cur = self.state.player.volume_percent;
                    let next =
                        next_volume_percent(&self.state.cfg.audio.volume_available_percent, cur);
                    p.send(PlayerCommand::SetVolumePercent(next));
                }
            }
            Action::VolumeDown => {
                if let Some(p) = self.player.as_ref() {
                    let cur = self.state.player.volume_percent;
                    let prev =
                        prev_volume_percent(&self.state.cfg.audio.volume_available_percent, cur);
                    p.send(PlayerCommand::SetVolumePercent(prev));
                }
            }

            Action::RescanLibrary => {
                if self.state.cfg.folders.is_empty() {
                    self.state.status = Some("no folders configured to scan".to_string());
                    return Ok(());
                }
                let req = self.active_folders_scan_request(ScanOrigin::RescanLibrary)?;
                self.request_scan(req);
                // If the scan spawner completes synchronously (tests), apply immediately.
                self.poll_scan_job_completion();
            }

            Action::SavePlaylists => {
                self.persistence
                    .save_playlists(&self.state.paths, &self.state.playlists)?;
                self.state.playlists_dirty = false;
                tracing::info!(
                    target: crate::logging::PERSIST_LOG_TARGET,
                    "playlists saved"
                );
                self.state.status = Some("playlists saved".to_string());
            }

            Action::CreatePlaylist { name } => {
                let name = name.trim();
                if name.is_empty() {
                    self.state.status = Some("playlist name must not be empty".to_string());
                    return Ok(());
                }
                let id = playlist_id(name);
                let p = Playlist {
                    id,
                    name: name.to_string(),
                    folders: self.state.cfg.folders.clone(),
                    extra: Default::default(),
                };
                p.validate()
                    .map_err(|msg| AppError::Config { message: msg })?;
                self.state.playlists.playlists.push(p);
                self.state.playlists_dirty = true;
                tracing::debug!(playlist_name = %name, "playlist created (unsaved)");
                self.state.status = Some("playlist created (unsaved)".to_string());
            }
            Action::RenamePlaylist { idx, name } => {
                let name = name.trim();
                if name.is_empty() {
                    self.state.status = Some("playlist name must not be empty".to_string());
                    return Ok(());
                }
                if let Some(p) = self.state.playlists.playlists.get_mut(idx) {
                    let old = p.name.clone();
                    p.name = name.to_string();
                    p.validate()
                        .map_err(|msg| AppError::Config { message: msg })?;
                    self.state.playlists_dirty = true;
                    tracing::debug!(from = %old, to = %name, "playlist renamed (unsaved)");
                    self.state.status = Some("playlist renamed (unsaved)".to_string());
                }
            }
            Action::DeletePlaylist { idx } => {
                if idx >= self.state.playlists.playlists.len() {
                    return Ok(());
                }
                let deleted_id = self.state.playlists.playlists[idx].id.clone();
                let deleted_name = self.state.playlists.playlists[idx].name.clone();
                self.state.playlists.playlists.remove(idx);
                if self.state.playlists.active.as_deref() == Some(deleted_id.as_str()) {
                    self.state.playlists.active = None;
                }
                self.state.playlists_dirty = true;
                tracing::debug!(
                    playlist_id = %deleted_id,
                    playlist_name = %deleted_name,
                    "playlist deleted (unsaved)"
                );
                self.state.status = Some("playlist deleted (unsaved)".to_string());
                if self.state.playlists_selected >= self.state.playlists.playlists.len() {
                    self.state.playlists_selected =
                        self.state.playlists.playlists.len().saturating_sub(1);
                }
            }
            Action::OverwritePlaylistWithCurrent { idx } => {
                let Some(p) = self.state.playlists.playlists.get_mut(idx) else {
                    return Ok(());
                };
                p.folders = self.state.cfg.folders.clone();
                let (playlist_id, playlist_name) = (p.id.clone(), p.name.clone());
                let _ = p; // end mutable borrow before immutable borrow below
                self.state.playlists_dirty = true;
                tracing::debug!(
                    playlist_id = %playlist_id,
                    playlist_name = %playlist_name,
                    "playlist overwritten (unsaved)"
                );
                self.state.status = Some("playlist overwritten (unsaved)".to_string());
            }
            Action::LoadPlaylist { idx } => {
                if let Some(p) = self.state.playlists.playlists.get(idx) {
                    let is_playing_or_paused =
                        self.state.player.status != crate::player::PlaybackStatus::Stopped;
                    let requested_source = PlaybackSource::Playlist(p.id.clone());

                    // Guard: if the user selects the playlist that is already playing, just
                    // navigate to Now Playing without restarting playback, queue, or rescanning.
                    if is_playing_or_paused
                        && self.state.playback_source.as_ref() == Some(&requested_source)
                    {
                        self.state.screen = Screen::NowPlaying;
                        self.state.status = None;
                        return Ok(());
                    }

                    // Defined behavior: if user loads a playlist during playback, we stop playback,
                    // swap folders immediately, rescan the library, and return to the main menu.
                    let stopped_playback =
                        self.state.player.status != crate::player::PlaybackStatus::Stopped;
                    if stopped_playback {
                        if let Some(ph) = self.player.as_ref() {
                            ph.send(PlayerCommand::Stop);
                        }
                        self.state.screen = Screen::MainMenu;
                    }

                    self.state.cfg.folders = p.folders.clone();
                    self.state.cfg = self.state.cfg.clone().normalized();
                    self.persistence
                        .save_config(&self.state.paths, &self.state.cfg)?;
                    self.state.playlists.active = Some(p.id.clone());
                    self.state.playlists_dirty = true;
                    tracing::debug!(
                        playlist_id = %p.id,
                        playlist_name = %p.name,
                        stopped_playback,
                        "playlist loaded (unsaved)"
                    );

                    // Proactively rescan so the library/queue view matches the newly active playlist.
                    let req = self.active_folders_scan_request(ScanOrigin::LoadPlaylist {
                        stopped_playback,
                    })?;
                    self.request_scan(req);
                    // If the scan spawner completes synchronously (tests), apply immediately.
                    self.poll_scan_job_completion();
                }
            }
        }
        Ok(())
    }

    fn active_folders_scan_request(&self, origin: ScanOrigin) -> AppResult<ScanRequest> {
        // Keep options-level min_size_bytes consistent with settings.min_size_kb (and ignore the
        // derived field to avoid staleness if callers modify min_size_kb directly in tests).
        let default_min_size_bytes = self
            .state
            .cfg
            .settings
            .min_size_kb
            .checked_mul(1024)
            .ok_or_else(|| AppError::Config {
                message: "settings.min_size_kb is too large".to_string(),
            })?;
        let folders = self
            .state
            .cfg
            .folders
            .iter()
            .map(|f| {
                let eff_kb = effective_min_size_kb_for_folder(f, &self.state.cfg.settings);
                let min_size_bytes = eff_kb.checked_mul(1024).ok_or_else(|| AppError::Config {
                    message: "effective min_size_kb is too large".to_string(),
                })?;
                Ok(FolderScanEntry {
                    path: f.path.clone(),
                    scan_depth: f.scan_depth,
                    min_size_bytes,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(ScanRequest {
            folders,
            opts: ScanOptions {
                supported_extensions: self.state.cfg.settings.supported_extensions.clone(),
                // Note: per-folder filtering uses `FolderScanEntry.min_size_bytes`.
                // This is retained for scan APIs that still rely on options-level min_size.
                min_size_bytes: default_min_size_bytes,
                allow_name_size_fallback_dedup: true,
                force_canonicalize_fail: false,
            },
            origin,
        })
    }

    fn load_queue_from_current_library(&mut self, start_index: usize) {
        if self.state.library.tracks.is_empty() {
            self.state.status = Some("library is empty".to_string());
            return;
        }
        let tracks = library_tracks_to_paths(&self.state.library);
        let safe_start = clamp_start_index(start_index, tracks.len());

        // TZ-004: remember how the current playback queue was created.
        self.state.playback_source = Some(PlaybackSource::from_active_playlist_or_folders(
            self.state.playlists.active.as_deref(),
            &self.state.cfg.folders,
        ));

        if let Some(p) = self.player.as_ref() {
            #[cfg(test)]
            {
                self.load_queue_commands_sent += 1;
            }
            p.send(PlayerCommand::LoadQueue {
                tracks,
                start_index: safe_start,
            });
        }
        self.state.screen = Screen::NowPlaying;
        self.state.status = None;
    }

    fn request_scan(&mut self, req: ScanRequest) {
        if self.scan_job.is_some() {
            // Policy: queue one pending scan (last write wins).
            self.pending_scan = Some(req);
            // Avoid overwriting higher-priority "play pending" status.
            if self.pending_play_from_library.is_none() {
                self.state.status =
                    Some("scan already running; queued another scan...".to_string());
            }
            return;
        }

        // If Play triggered a scan, reflect "play pending" immediately even though we don't set
        // `pending_play_from_library` (queue load is driven by scan origin).
        if matches!(req.origin, ScanOrigin::PlayFromLibrary { .. })
            || self.pending_play_from_library.is_some()
        {
            self.state.status = Some("Scanning... (play pending)".to_string());
        } else {
            self.state.status = Some("Scanning...".to_string());
        }
        let mut job = self.scan_spawner.spawn_scan(req.folders, req.opts);
        job.origin = req.origin;
        self.scan_job = Some(job);
    }

    fn poll_scan_job_completion(&mut self) {
        // Non-blocking completion polling.
        let Some(mut job) = self.scan_job.take() else {
            // No active scan; maybe a pending one?
            if let Some(req) = self.pending_scan.take() {
                self.request_scan(req);
            }
            return;
        };

        match job.rx.try_recv() {
            Ok(index) => {
                // Join thread best-effort now that it should be done.
                if let Some(h) = job.handle.take() {
                    let _ = h.join();
                }
                // Background scans can be superseded by newer debounced changes; ignore stale
                // completion results so they don't overwrite newer state.
                if let ScanOrigin::BackgroundRescan { token } = job.origin {
                    if token != self.latest_bg_scan_token {
                        // Join already happened above; just continue to next pending scan.
                        if let Some(req) = self.pending_scan.take() {
                            self.request_scan(req);
                        }
                        return;
                    }
                }
                self.apply_scan_result(job.origin, index);

                // FIX-004: If Play was requested during an active scan, complete it immediately
                // after the scan finishes, using the freshly updated index.
                if let Some(start_index) = self.pending_play_from_library.take() {
                    self.load_queue_from_current_library(start_index);
                }

                // Start pending scan (if any) immediately after finishing.
                if let Some(req) = self.pending_scan.take() {
                    self.request_scan(req);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Still running.
                self.scan_job = Some(job);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                // Worker died; surface and allow future scans.
                self.state.status = Some("scan failed: worker disconnected".to_string());
                // Join best-effort.
                if let Some(h) = job.handle.take() {
                    if h.is_finished() {
                        let _ = h.join();
                    }
                }
                if let Some(req) = self.pending_scan.take() {
                    self.request_scan(req);
                }
            }
        }
    }

    fn persist_index_best_effort(&self, index: &crate::indexer::LibraryIndex) {
        let issues = index.report.issues.len();
        let tracks = index.tracks.len();

        // Best-effort cache persistence under data/cache/.
        if let Err(e) = indexer::io::save(&self.state.paths, index) {
            tracing::warn!(
                target: crate::logging::PERSIST_LOG_TARGET,
                error = %e,
                "failed to persist index cache"
            );
        }
        // Best-effort runtime state tracking under data/state.yaml.
        if let Ok(mut st) = app_state_file::load_or_create(&self.state.paths) {
            st.last_index = Some(app_state_file::LastIndexSummary {
                tracks_total: tracks,
                issues_total: issues,
            });
            if let Err(e) = app_state_file::save(&self.state.paths, &st) {
                tracing::warn!(
                    target: crate::logging::PERSIST_LOG_TARGET,
                    error = %e,
                    "failed to persist state.yaml"
                );
            }
        }
    }

    fn apply_scan_result(&mut self, origin: ScanOrigin, index: crate::indexer::LibraryIndex) {
        let issues = index.report.issues.len();
        let tracks = index.tracks.len();

        self.persist_index_best_effort(&index);
        self.state.library = index;

        match origin {
            ScanOrigin::RescanLibrary => {
                self.state.status =
                    Some(format!("scan complete: {tracks} tracks (issues: {issues})"));

                // FIX-004: If Play was requested during a rescan, the completion handler will
                // immediately load the queue from the freshly updated index. In that case, skip
                // the rescan "refresh while playing" branch to avoid issuing a second LoadQueue.
                if self.pending_play_from_library.is_some() {
                    return;
                }

                // If we're currently playing from the library, refresh the player queue to match
                // the new index. This avoids odd states when files disappear after a rescan.
                if self.state.player.status != crate::player::PlaybackStatus::Stopped {
                    let new_tracks = library_tracks_to_paths(&self.state.library);
                    if new_tracks.is_empty() {
                        if let Some(p) = self.player.as_ref() {
                            p.send(PlayerCommand::Stop);
                        }
                        self.state.screen = Screen::MainMenu;
                        self.state.status = Some(
                            "scan complete: library is now empty; playback stopped".to_string(),
                        );
                        return;
                    }

                    let current_path = self.state.player.current_path.clone();
                    let current_index = current_path
                        .as_ref()
                        .and_then(|cur| find_track_index_by_path(&new_tracks, cur));

                    if let (Some(_cur_path), Some(idx)) = (current_path, current_index) {
                        let selected_current_path = new_tracks[idx].clone();
                        if let Some(p) = self.player.as_ref() {
                            p.send(PlayerCommand::ResyncQueueAfterLibraryChange {
                                tracks: new_tracks,
                                // Use the exact refreshed-library `PathBuf` to avoid Windows
                                // normalization differences causing strict equality mismatches
                                // in the player.
                                current_path: selected_current_path,
                            });
                        }
                        self.state.status = Some(
                            "scan complete: queue resynced (preserved current track)".to_string(),
                        );
                        return;
                    }

                    // Current file no longer exists: fall back to reloading a new queue.
                    // Spec: shuffle off → start at 0; shuffle on → uniform random index.
                    let start_index = if self.state.cfg.settings.shuffle {
                        self.random_start_index(new_tracks.len())
                    } else {
                        0
                    };
                    let safe_start = clamp_start_index(start_index, new_tracks.len());
                    if let Some(p) = self.player.as_ref() {
                        #[cfg(test)]
                        {
                            self.load_queue_commands_sent += 1;
                        }
                        p.send(PlayerCommand::LoadQueue {
                            tracks: new_tracks,
                            start_index: safe_start,
                        });
                    }
                    self.state.status = Some("scan complete: queue refreshed".to_string());
                }
            }
            ScanOrigin::PlayFromLibrary { start_index } => {
                // The Play action guarantees scan-before-load, so the queue reflects reality.
                if self.state.library.tracks.is_empty() {
                    self.state.status = Some("scan complete: library is empty".to_string());
                    self.state.screen = Screen::MainMenu;
                    return;
                }
                self.load_queue_from_current_library(start_index);
            }
            ScanOrigin::LoadPlaylist { stopped_playback } => {
                if self.state.cfg.folders.is_empty() {
                    self.state.status =
                        Some("playlist loaded: it has no folders; library is empty".to_string());
                } else {
                    let stopped_suffix = if stopped_playback {
                        " (playback stopped)"
                    } else {
                        ""
                    };
                    self.state.status = Some(format!(
                        "playlist loaded{stopped_suffix}; scan: {tracks} tracks (issues: {issues})"
                    ));
                }
            }
            ScanOrigin::BackgroundRescan { token: _ } => {
                // Quiet refresh: keep UI responsive; status is left unchanged.
            }
        }
    }

    fn drain_player_events(&mut self) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        while let Some(evt) = p.try_recv() {
            match evt {
                PlayerEvent::Snapshot(s) => {
                    // Option A (FIX-006 semantics): snapshots may explicitly clear last_error.
                    self.state.last_error = s.last_error.clone();
                    self.state.player = s
                }
                PlayerEvent::Error(msg) => {
                    self.state.last_error = Some(msg.clone());
                    self.state.status = Some(msg);
                }
                PlayerEvent::ShutdownAck => {}
            }
        }
    }

    pub fn shutdown_player(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        let Some(p) = self.player.take() else {
            return Ok(());
        };
        p.shutdown_and_join(timeout)
    }
}

fn playlist_id(name: &str) -> String {
    let mut h = DefaultHasher::new();
    name.to_lowercase().hash(&mut h);
    let v = h.finish();
    format!("p{:016x}", v)
}

fn next_volume_percent(available: &[u8], current: u8) -> u8 {
    if available.is_empty() {
        return current;
    }
    match available.binary_search(&current) {
        Ok(idx) => available
            .get(idx.saturating_add(1))
            .copied()
            .unwrap_or(current),
        Err(insert_idx) => available.get(insert_idx).copied().unwrap_or(current),
    }
}

fn prev_volume_percent(available: &[u8], current: u8) -> u8 {
    if available.is_empty() {
        return current;
    }
    match available.binary_search(&current) {
        Ok(idx) => available
            .get(idx.saturating_sub(1))
            .copied()
            .unwrap_or(current),
        Err(insert_idx) => {
            if insert_idx == 0 {
                current
            } else {
                available.get(insert_idx - 1).copied().unwrap_or(current)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::config::{AppConfig, RepeatMode};
    use crate::error::AppError;
    use crate::indexer::{LibraryIndex, TrackEntry, TrackId};
    use crate::paths::AppPaths;
    use crate::player::PlaybackStatus;
    use crate::playlists::{Playlist, PlaylistsFile};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::cell::RefCell;
    use std::fs;
    use std::sync::mpsc::Sender;

    #[derive(Default)]
    struct InlineScanSpawner;

    impl ScanSpawner for InlineScanSpawner {
        fn spawn_scan(&self, folders: Vec<FolderScanEntry>, opts: ScanOptions) -> ScanJobState {
            let (tx, rx) = mpsc::channel::<crate::indexer::LibraryIndex>();
            let started_at = Instant::now();
            let index = indexer::scan::scan_library_folders(&folders, &opts);
            let _ = tx.send(index);
            ScanJobState {
                origin: ScanOrigin::RescanLibrary,
                started_at,
                rx,
                handle: None,
            }
        }
    }

    #[derive(Default)]
    struct MockPersistence {
        config_writes: RefCell<usize>,
        playlists_writes: RefCell<usize>,
        last_saved_cfg: RefCell<Option<AppConfig>>,
        last_saved_playlists: RefCell<Option<PlaylistsFile>>,
    }

    impl MockPersistence {
        fn config_writes(&self) -> usize {
            *self.config_writes.borrow()
        }

        fn playlists_writes(&self) -> usize {
            *self.playlists_writes.borrow()
        }
    }

    impl Persistence for MockPersistence {
        fn save_config(&self, _paths: &AppPaths, cfg: &AppConfig) -> AppResult<()> {
            *self.config_writes.borrow_mut() += 1;
            *self.last_saved_cfg.borrow_mut() = Some(cfg.clone());
            Ok(())
        }

        fn save_playlists(&self, _paths: &AppPaths, pls: &PlaylistsFile) -> AppResult<()> {
            *self.playlists_writes.borrow_mut() += 1;
            *self.last_saved_playlists.borrow_mut() = Some(pls.clone());
            Ok(())
        }
    }

    fn paths_for(dir: &std::path::Path) -> AppPaths {
        let base_dir = dir.to_path_buf();
        let data_dir = base_dir.join("data");
        AppPaths {
            base_dir,
            cache_dir: data_dir.join("cache"),
            logs_dir: data_dir.join("logs"),
            config_path: data_dir.join("config.yaml"),
            playlists_path: data_dir.join("playlists.yaml"),
            state_path: data_dir.join("state.yaml"),
            data_dir,
        }
    }

    fn app_with_mock(
        paths: AppPaths,
        cfg: AppConfig,
        playlists: PlaylistsFile,
    ) -> (TuiApp, std::rc::Rc<MockPersistence>) {
        let mock = std::rc::Rc::new(MockPersistence::default());
        let app = TuiApp::new_with_deps(
            paths,
            cfg,
            playlists,
            Box::new(RcPersistence(mock.clone())),
            Box::new(InlineScanSpawner),
        );
        (app, mock)
    }

    fn app_with_mock_and_scan_spawner(
        paths: AppPaths,
        cfg: AppConfig,
        playlists: PlaylistsFile,
        scan_spawner: Box<dyn ScanSpawner>,
    ) -> (TuiApp, std::rc::Rc<MockPersistence>) {
        let mock = std::rc::Rc::new(MockPersistence::default());
        let app = TuiApp::new_with_deps(
            paths,
            cfg,
            playlists,
            Box::new(RcPersistence(mock.clone())),
            scan_spawner,
        );
        (app, mock)
    }

    fn write_dummy_file(path: &std::path::Path, size_bytes: usize) {
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        let data = vec![0u8; size_bytes.max(1)];
        fs::write(path, data).unwrap();
    }

    #[derive(Clone, Default)]
    struct ManualScanControl {
        tx: std::rc::Rc<RefCell<Option<Sender<LibraryIndex>>>>,
    }

    #[derive(Clone, Default)]
    struct ManualScanSpawner {
        control: ManualScanControl,
    }

    impl ManualScanSpawner {
        fn take_sender(&self) -> Sender<LibraryIndex> {
            self.control
                .tx
                .borrow_mut()
                .take()
                .expect("manual scan sender not available")
        }
    }

    impl ScanSpawner for ManualScanSpawner {
        fn spawn_scan(&self, _folders: Vec<FolderScanEntry>, _opts: ScanOptions) -> ScanJobState {
            let (tx, rx) = mpsc::channel::<crate::indexer::LibraryIndex>();
            *self.control.tx.borrow_mut() = Some(tx);
            ScanJobState {
                origin: ScanOrigin::RescanLibrary,
                started_at: Instant::now(),
                rx,
                handle: None,
            }
        }
    }

    struct RcPersistence(std::rc::Rc<MockPersistence>);

    impl Persistence for RcPersistence {
        fn save_config(&self, paths: &AppPaths, cfg: &AppConfig) -> AppResult<()> {
            self.0.save_config(paths, cfg)
        }
        fn save_playlists(&self, paths: &AppPaths, pls: &PlaylistsFile) -> AppResult<()> {
            self.0.save_playlists(paths, pls)
        }
    }

    #[test]
    fn add_folder_triggers_config_save_and_normalizes() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::AddFolder("  C:\\Music  ".to_string()))
            .unwrap();

        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry::new("C:\\Music".to_string())]
        );
        assert_eq!(mock.config_writes(), 1);
        assert_eq!(mock.playlists_writes(), 0);
        assert_eq!(
            mock.last_saved_cfg.borrow().as_ref().unwrap().folders,
            vec![FolderEntry::new("C:\\Music".to_string())]
        );
        assert_eq!(app.state.status.as_deref(), Some("folder added and saved"));
    }

    #[test]
    fn remove_folder_out_of_range_is_noop_and_does_not_save() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new("A".to_string())];
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::RemoveFolderAt(99)).unwrap();

        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry::new("A".to_string())]
        );
        assert_eq!(mock.config_writes(), 0);
    }

    #[test]
    fn remove_folder_adjusts_selection_and_saves() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![
            FolderEntry::new("A".to_string()),
            FolderEntry::new("B".to_string()),
        ];
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);
        app.state.main_selected_folder = 1;

        app.apply(Action::RemoveFolderAt(1)).unwrap();

        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry::new("A".to_string())]
        );
        assert_eq!(app.state.main_selected_folder, 0);
        assert_eq!(mock.config_writes(), 1);
        assert_eq!(
            app.state.status.as_deref(),
            Some("folder removed and saved")
        );
    }

    #[test]
    fn settings_actions_trigger_config_save() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::SetMinSizeKb(123)).unwrap();
        assert_eq!(mock.config_writes(), 1);
        assert_eq!(app.state.cfg.settings.min_size_kb, 123);
        assert_eq!(app.state.cfg.settings.min_size_bytes, 123 * 1024);

        app.apply(Action::ToggleShuffle).unwrap();
        assert_eq!(mock.config_writes(), 2);
        assert!(app.state.cfg.settings.shuffle);

        let before = app.state.cfg.settings.repeat;
        app.apply(Action::CycleRepeat).unwrap();
        assert_eq!(mock.config_writes(), 3);
        assert_ne!(app.state.cfg.settings.repeat, before);
    }

    #[test]
    fn toggle_folder_root_only_flips_and_saves() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry {
            path: "C:\\Music".to_string(),
            scan_depth: crate::config::ScanDepth::RootOnly,
            custom_min_size_kb: None,
        }];
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::ToggleFolderRootOnlyAt(0)).unwrap();

        assert_eq!(mock.config_writes(), 1);
        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry {
                path: "C:\\Music".to_string(),
                scan_depth: crate::config::ScanDepth::OneLevel,
                custom_min_size_kb: None,
            }]
        );
        assert_eq!(app.state.status.as_deref(), Some("scan depth: one-level"));
    }

    #[test]
    fn toggle_folder_root_only_cycles_root_only_one_level_recursive_root_only_and_saves_each_time()
    {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry {
            path: "C:\\Music".to_string(),
            scan_depth: crate::config::ScanDepth::RootOnly,
            custom_min_size_kb: None,
        }];
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::ToggleFolderRootOnlyAt(0)).unwrap();
        assert_eq!(mock.config_writes(), 1);
        assert_eq!(
            app.state.cfg.folders[0].scan_depth,
            crate::config::ScanDepth::OneLevel
        );
        assert_eq!(app.state.status.as_deref(), Some("scan depth: one-level"));

        app.apply(Action::ToggleFolderRootOnlyAt(0)).unwrap();
        assert_eq!(mock.config_writes(), 2);
        assert_eq!(
            app.state.cfg.folders[0].scan_depth,
            crate::config::ScanDepth::Recursive
        );
        assert_eq!(app.state.status.as_deref(), Some("scan depth: recursive"));

        app.apply(Action::ToggleFolderRootOnlyAt(0)).unwrap();
        assert_eq!(mock.config_writes(), 3);
        assert_eq!(
            app.state.cfg.folders[0].scan_depth,
            crate::config::ScanDepth::RootOnly
        );
        assert_eq!(app.state.status.as_deref(), Some("scan depth: root-only"));
    }

    #[test]
    fn toggle_folder_root_only_updates_only_selected_index_and_saves() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![
            FolderEntry {
                path: "A".to_string(),
                scan_depth: crate::config::ScanDepth::RootOnly,
                custom_min_size_kb: None,
            },
            FolderEntry {
                path: "B".to_string(),
                scan_depth: crate::config::ScanDepth::Recursive,
                custom_min_size_kb: None,
            },
        ];
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);
        app.state.main_selected_folder = 1;

        let idx = app.state.main_selected_folder;
        app.apply(Action::ToggleFolderRootOnlyAt(idx)).unwrap();

        assert_eq!(mock.config_writes(), 1);
        assert_eq!(
            app.state.cfg.folders,
            vec![
                FolderEntry {
                    path: "A".to_string(),
                    scan_depth: crate::config::ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "B".to_string(),
                    scan_depth: crate::config::ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                }
            ]
        );
        assert_eq!(app.state.status.as_deref(), Some("scan depth: root-only"));
    }

    #[test]
    fn create_playlist_marks_dirty_and_copies_current_folders() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![
            FolderEntry::new("A".to_string()),
            FolderEntry::new("B".to_string()),
        ];
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::CreatePlaylist {
            name: " My Mix ".to_string(),
        })
        .unwrap();

        assert_eq!(mock.playlists_writes(), 0);
        assert_eq!(app.state.playlists.playlists.len(), 1);
        assert_eq!(app.state.playlists.playlists[0].name, "My Mix");
        assert_eq!(
            app.state.playlists.playlists[0].folders,
            vec![
                FolderEntry::new("A".to_string()),
                FolderEntry::new("B".to_string())
            ]
        );
        assert!(app.state.playlists_dirty);
        assert!(mock.last_saved_playlists.borrow().is_none());
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist created (unsaved)")
        );
    }

    #[test]
    fn rename_playlist_saves_when_index_valid() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "Old".to_string(),
            folders: vec![],
            extra: Default::default(),
        });
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::RenamePlaylist {
            idx: 0,
            name: "New".to_string(),
        })
        .unwrap();

        assert_eq!(app.state.playlists.playlists[0].name, "New");
        assert_eq!(mock.playlists_writes(), 0);
        assert!(app.state.playlists_dirty);
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist renamed (unsaved)")
        );
    }

    #[test]
    fn rename_playlist_out_of_range_does_not_save() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::RenamePlaylist {
            idx: 42,
            name: "New".to_string(),
        })
        .unwrap();

        assert_eq!(mock.playlists_writes(), 0);
    }

    #[test]
    fn delete_playlist_clears_active_and_marks_dirty() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let mut pls = PlaylistsFile::default();
        pls.active = Some("p1".to_string());
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "One".to_string(),
            folders: vec![],
            extra: Default::default(),
        });
        pls.playlists.push(Playlist {
            id: "p2".to_string(),
            name: "Two".to_string(),
            folders: vec![],
            extra: Default::default(),
        });
        let (mut app, mock) = app_with_mock(paths, cfg, pls);
        app.state.playlists_selected = 1;

        app.apply(Action::DeletePlaylist { idx: 0 }).unwrap();

        assert_eq!(mock.playlists_writes(), 0);
        assert_eq!(app.state.playlists.playlists.len(), 1);
        assert_eq!(app.state.playlists.active, None);
        assert!(app.state.playlists_dirty);
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist deleted (unsaved)")
        );
        assert_eq!(app.state.playlists_selected, 0);
    }

    #[test]
    fn save_playlists_persists_and_clears_dirty_flag() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::CreatePlaylist {
            name: "Mix".to_string(),
        })
        .unwrap();
        assert!(
            app.state.playlists_dirty,
            "setup: playlist mutation should set dirty"
        );
        assert_eq!(mock.playlists_writes(), 0, "no autosave on mutation");

        app.apply(Action::SavePlaylists).unwrap();

        assert_eq!(
            mock.playlists_writes(),
            1,
            "explicit save should persist once"
        );
        assert!(!app.state.playlists_dirty, "save should clear dirty");
        assert_eq!(app.state.status.as_deref(), Some("playlists saved"));
        assert!(mock.last_saved_playlists.borrow().is_some());
    }

    #[test]
    fn tick_does_not_autosave_playlists_when_dirty() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::CreatePlaylist {
            name: "Mix".to_string(),
        })
        .unwrap();
        assert!(app.state.playlists_dirty);
        let before = mock.playlists_writes();

        // Regression guard: no autosave on tick/draw loop.
        let _ = app.tick().unwrap();

        assert_eq!(
            mock.playlists_writes(),
            before,
            "tick must not persist playlists automatically"
        );
    }

    #[test]
    fn create_playlist_empty_name_sets_status_and_does_not_save() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::CreatePlaylist {
            name: "   ".to_string(),
        })
        .unwrap();

        assert_eq!(mock.playlists_writes(), 0);
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist name must not be empty")
        );
    }

    #[test]
    fn load_playlist_swaps_folders_sets_active_and_saves_config_only() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new("X".to_string())];
        cfg.settings.repeat = RepeatMode::Off;

        // Create real folders so the indexer doesn't report MissingFolder issues.
        let a_dir = td.path().join("A");
        let b_dir = td.path().join("B");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();
        let a = a_dir.to_string_lossy().to_string();
        let b = b_dir.to_string_lossy().to_string();

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "One".to_string(),
            folders: vec![
                FolderEntry::new(a.clone()),
                FolderEntry::new(a),
                FolderEntry::new(b),
            ],
            extra: Default::default(),
        });
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        // folders swapped + normalized (dedup keep order)
        assert_eq!(
            app.state.cfg.folders,
            vec![
                FolderEntry::new(a_dir.to_string_lossy().to_string()),
                FolderEntry::new(b_dir.to_string_lossy().to_string())
            ]
        );
        assert_eq!(app.state.playlists.active.as_deref(), Some("p1"));
        assert_eq!(mock.config_writes(), 1);
        assert_eq!(mock.playlists_writes(), 0);
        assert!(app.state.playlists_dirty);
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist loaded; scan: 0 tracks (issues: 0)")
        );
    }

    #[test]
    fn play_navigates_to_now_playing_and_back_to_main_menu() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::Navigate(Screen::NowPlaying)).unwrap();
        assert_eq!(app.state.screen, Screen::NowPlaying);

        let a = app
            .on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap()
            .unwrap();
        app.apply(a).unwrap();
        assert_eq!(app.state.screen, Screen::MainMenu);
    }

    #[test]
    fn library_tracks_to_paths_preserves_order_and_paths() {
        let mut lib = LibraryIndex::default();
        lib.tracks = vec![
            TrackEntry {
                id: TrackId(1),
                path: std::path::PathBuf::from("A.ogg"),
                rel_path: None,
                size_bytes: 1,
            },
            TrackEntry {
                id: TrackId(2),
                path: std::path::PathBuf::from("B.ogg"),
                rel_path: None,
                size_bytes: 1,
            },
        ];

        let paths = library_tracks_to_paths(&lib);
        assert_eq!(
            paths,
            vec![
                std::path::PathBuf::from("A.ogg"),
                std::path::PathBuf::from("B.ogg")
            ]
        );
    }

    #[test]
    fn clamp_start_index_handles_empty_and_caps_to_last() {
        assert_eq!(clamp_start_index(0, 0), 0);
        assert_eq!(clamp_start_index(999, 0), 0);
        assert_eq!(clamp_start_index(0, 3), 0);
        assert_eq!(clamp_start_index(2, 3), 2);
        assert_eq!(clamp_start_index(3, 3), 2);
        assert_eq!(clamp_start_index(999, 3), 2);
    }

    #[test]
    fn np_002_find_track_index_by_path_uses_canonicalize_fallback_for_equivalent_paths() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path();
        let sub = base.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let file = base.join("a.ogg");
        write_dummy_file(&file, 16);

        let direct = std::fs::canonicalize(&file).unwrap();
        let needle = sub.join("..").join("a.ogg");
        assert_ne!(
            direct, needle,
            "test setup requires distinct path spellings pointing to same file"
        );

        let tracks = vec![direct];
        let idx = find_track_index_by_path(&tracks, &needle);
        assert_eq!(idx, Some(0));
    }

    #[test]
    fn t2_scan_options_min_size_bytes_is_derived_from_min_size_kb_not_stale_min_size_bytes_field() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 5;
        cfg.settings.min_size_bytes = 999_999; // must be ignored at scan boundary
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let (app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());

        let req = app
            .active_folders_scan_request(ScanOrigin::RescanLibrary)
            .unwrap();
        assert_eq!(req.opts.min_size_bytes, 5 * 1024);
    }

    #[test]
    fn t2_scan_options_min_size_kb_overflow_returns_config_error() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = u64::MAX;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let (app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());

        let err = app
            .active_folders_scan_request(ScanOrigin::RescanLibrary)
            .unwrap_err();
        match err {
            AppError::Config { message } => {
                assert!(
                    message.contains("settings.min_size_kb is too large"),
                    "message was: {message}"
                );
            }
            other => panic!("expected AppError::Config, got {other:?}"),
        }
    }

    #[test]
    fn ost_008_load_playlist_while_playing_stops_and_rescans_non_empty() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music_a");
        let track = music_dir.join("t1.ogg");
        write_dummy_file(&track, 16);

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("X".to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "A".to_string(),
            folders: vec![FolderEntry::new(music_dir.to_string_lossy().to_string())],
            extra: Default::default(),
        });
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        assert_eq!(app.state.screen, Screen::MainMenu);
        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry::new(music_dir.to_string_lossy().to_string())]
        );
        assert_eq!(app.state.playlists.active.as_deref(), Some("p1"));
        assert_eq!(app.state.library.tracks.len(), 1);
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist loaded (playback stopped); scan: 1 tracks (issues: 0)")
        );
    }

    #[test]
    fn ost_008_load_playlist_while_playing_handles_empty_playlist_folders() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("X".to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "Empty".to_string(),
            folders: vec![],
            extra: Default::default(),
        });
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        assert_eq!(app.state.screen, Screen::MainMenu);
        assert!(app.state.cfg.folders.is_empty());
        assert_eq!(app.state.library.tracks.len(), 0);
        assert_eq!(app.state.playlists.active.as_deref(), Some("p1"));
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist loaded: it has no folders; library is empty")
        );
    }

    #[test]
    fn tz_002_load_playlist_that_is_already_playing_does_not_restart() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music_a");
        std::fs::create_dir_all(&music_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "A".to_string(),
            folders: vec![FolderEntry::new(music_dir.to_string_lossy().to_string())],
            extra: Default::default(),
        });

        let (mut app, mock) = app_with_mock(paths, cfg, pls);
        // Replace the real player handle with a controllable one so we can assert
        // that no player commands are emitted when the guard triggers.
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::Playlists;
        app.state.playlists.active = Some("p1".to_string());
        app.state.player.status = PlaybackStatus::Playing;
        app.state.playback_source = Some(PlaybackSource::Playlist("p1".to_string()));
        let before_cfg_writes = mock.config_writes();
        let before_pls_writes = mock.playlists_writes();
        let before_load_queue_commands_sent = app.load_queue_commands_sent;

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(mock.config_writes(), before_cfg_writes);
        assert_eq!(mock.playlists_writes(), before_pls_writes);
        assert_eq!(app.state.status, None);
        assert!(
            cmd_rx.try_recv().is_err(),
            "guard should not emit any PlayerCommand (no Stop/LoadQueue/etc)"
        );
        assert!(
            app.scan_job.is_none() && app.pending_scan.is_none(),
            "guard should not start or queue a scan"
        );
        assert_eq!(
            app.load_queue_commands_sent, before_load_queue_commands_sent,
            "guard should not load/reload queue"
        );
    }

    #[test]
    fn tz_004_guard_triggers_when_same_playlist_and_player_paused() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music_a");
        std::fs::create_dir_all(&music_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "A".to_string(),
            folders: vec![FolderEntry::new(music_dir.to_string_lossy().to_string())],
            extra: Default::default(),
        });

        let (mut app, mock) = app_with_mock(paths, cfg, pls);
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::Playlists;
        app.state.playlists.active = Some("p1".to_string());
        app.state.player.status = PlaybackStatus::Paused;
        app.state.playback_source = Some(PlaybackSource::Playlist("p1".to_string()));
        let before_cfg_writes = mock.config_writes();
        let before_pls_writes = mock.playlists_writes();
        let before_load_queue_commands_sent = app.load_queue_commands_sent;

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(mock.config_writes(), before_cfg_writes);
        assert_eq!(mock.playlists_writes(), before_pls_writes);
        assert_eq!(app.state.status, None);
        assert!(
            cmd_rx.try_recv().is_err(),
            "guard should not emit any PlayerCommand (no Stop/LoadQueue/etc)"
        );
        assert!(
            app.scan_job.is_none() && app.pending_scan.is_none(),
            "guard should not start or queue a scan"
        );
        assert_eq!(
            app.load_queue_commands_sent, before_load_queue_commands_sent,
            "guard should not load/reload queue"
        );
    }

    #[test]
    fn tz_002_load_playlist_active_but_player_stopped_loads_normally() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        // Create a real folder so the indexer doesn't report MissingFolder issues.
        let music_dir = td.path().join("music_a");
        std::fs::create_dir_all(&music_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new("X".to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "A".to_string(),
            folders: vec![FolderEntry::new(music_dir.to_string_lossy().to_string())],
            extra: Default::default(),
        });

        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        // Use a controllable player and assert LoadPlaylist doesn't emit Stop when already stopped.
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::Playlists;
        app.state.playlists.active = Some("p1".to_string());
        app.state.player.status = PlaybackStatus::Stopped;

        let before_cfg_writes = mock.config_writes();
        let before_pls_writes = mock.playlists_writes();

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        // Normal path: swap folders, save config+playlists, rescan (InlineScanSpawner completes),
        // and set a "playlist loaded; scan: ..." status.
        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry::new(music_dir.to_string_lossy().to_string())]
        );
        assert_eq!(app.state.playlists.active.as_deref(), Some("p1"));
        assert_eq!(mock.config_writes(), before_cfg_writes + 1);
        assert_eq!(mock.playlists_writes(), before_pls_writes);
        assert!(app.state.playlists_dirty);
        assert!(
            app.scan_job.is_none(),
            "InlineScanSpawner should complete scan synchronously"
        );
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist loaded; scan: 0 tracks (issues: 0)")
        );

        // No Stop should be issued when already stopped, and LoadQueue isn't part of playlist load.
        assert!(
            cmd_rx.try_recv().is_err(),
            "stopped playback should not emit Stop/LoadQueue during playlist load"
        );
    }

    #[test]
    fn tz_002_guard_does_not_trigger_when_active_id_matches_but_folders_differ() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let a_dir = td.path().join("A");
        let b_dir = td.path().join("B");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new(a_dir.to_string_lossy().to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "Playlist".to_string(),
            folders: vec![FolderEntry::new(b_dir.to_string_lossy().to_string())],
            extra: Default::default(),
        });

        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::Playlists;
        app.state.playlists.active = Some("p1".to_string());
        app.state.player.status = PlaybackStatus::Playing;
        app.state.playback_source = Some(PlaybackSource::FoldersHash(123));

        let before_cfg_writes = mock.config_writes();
        let before_pls_writes = mock.playlists_writes();

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry::new(b_dir.to_string_lossy().to_string())]
        );
        assert_eq!(app.state.playlists.active.as_deref(), Some("p1"));
        assert_eq!(mock.config_writes(), before_cfg_writes + 1);
        assert_eq!(mock.playlists_writes(), before_pls_writes);
        assert!(app.state.playlists_dirty);

        // Because playback was active, it should have stopped playback.
        assert_eq!(cmd_rx.try_recv().unwrap(), PlayerCommand::Stop);
    }

    #[test]
    fn tz_004_guard_does_not_trigger_when_playback_source_differs_even_if_paused() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let a_dir = td.path().join("A");
        let b_dir = td.path().join("B");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new(a_dir.to_string_lossy().to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "Playlist".to_string(),
            folders: vec![FolderEntry::new(b_dir.to_string_lossy().to_string())],
            extra: Default::default(),
        });

        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::Playlists;
        app.state.playlists.active = Some("p1".to_string());
        app.state.player.status = PlaybackStatus::Paused;
        // Different source => guard must NOT trigger.
        app.state.playback_source = Some(PlaybackSource::FoldersHash(123));

        let before_cfg_writes = mock.config_writes();
        let before_pls_writes = mock.playlists_writes();

        app.apply(Action::LoadPlaylist { idx: 0 }).unwrap();

        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry::new(b_dir.to_string_lossy().to_string())]
        );
        assert_eq!(app.state.playlists.active.as_deref(), Some("p1"));
        assert_eq!(mock.config_writes(), before_cfg_writes + 1);
        assert_eq!(mock.playlists_writes(), before_pls_writes);
        assert!(app.state.playlists_dirty);
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::Stop,
            "paused is still active playback; normal path should stop playback"
        );
    }

    #[test]
    fn tz_004_load_queue_sets_playback_source_to_playlist_when_active_playlist_is_set() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new("A".to_string())];

        let mut pls = PlaylistsFile::default();
        pls.active = Some("p1".to_string());

        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        // Replace the real player handle with a controllable one (queue load will emit a command).
        let (cmd_tx, _cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        // Provide a non-empty library so load_queue_from_current_library proceeds.
        app.state.library.tracks = vec![TrackEntry {
            id: TrackId(1),
            path: std::path::PathBuf::from("A.ogg"),
            rel_path: None,
            size_bytes: 1,
        }];

        app.load_queue_from_current_library(0);

        assert_eq!(
            app.state.playback_source,
            Some(PlaybackSource::Playlist("p1".to_string()))
        );
    }

    #[test]
    fn tz_004_load_queue_sets_playback_source_to_folders_hash_when_no_active_playlist() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.folders = vec![
            FolderEntry::new("A".to_string()),
            FolderEntry::new("A".to_string()), // duplicates should not affect identity
            FolderEntry::new("B".to_string()),
        ];

        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        // Replace the real player handle with a controllable one (queue load will emit a command).
        let (cmd_tx, _cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        // Provide a non-empty library so load_queue_from_current_library proceeds.
        app.state.library.tracks = vec![TrackEntry {
            id: TrackId(1),
            path: std::path::PathBuf::from("A.ogg"),
            rel_path: None,
            size_bytes: 1,
        }];

        app.load_queue_from_current_library(0);

        let expected =
            PlaybackSource::from_active_playlist_or_folders(None, &app.state.cfg.folders);
        assert_eq!(app.state.playback_source, Some(expected));
        assert!(
            matches!(
                app.state.playback_source,
                Some(PlaybackSource::FoldersHash(_))
            ),
            "expected folders-hash source when no active playlist"
        );
    }

    #[test]
    fn ost_008_rescan_while_playing_empty_library_stops_and_returns_to_main_menu() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let empty_dir = td.path().join("empty");
        fs::create_dir_all(&empty_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new(empty_dir.to_string_lossy().to_string())];

        let (mut app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());

        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;
        app.state.player.current_path = Some(empty_dir.join("does_not_matter.ogg"));

        app.apply(Action::RescanLibrary).unwrap();

        assert_eq!(app.state.library.tracks.len(), 0);
        assert_eq!(app.state.screen, Screen::MainMenu);
        assert_eq!(
            app.state.status.as_deref(),
            Some("scan complete: library is now empty; playback stopped")
        );
    }

    #[test]
    fn ost_008_rescan_while_playing_refreshes_queue_when_library_non_empty() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music");
        let t1 = music_dir.join("a.ogg");
        let t2 = music_dir.join("b.ogg");
        write_dummy_file(&t1, 16);
        write_dummy_file(&t2, 16);

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let (mut app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());

        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;
        app.state.player.current_path = Some(t2.clone());
        let before_load_queue_commands_sent = app.load_queue_commands_sent;

        app.apply(Action::RescanLibrary).unwrap();

        assert_eq!(app.state.library.tracks.len(), 2);
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.state.status.as_deref(),
            Some("scan complete: queue resynced (preserved current track)")
        );
        assert_eq!(
            app.load_queue_commands_sent, before_load_queue_commands_sent,
            "resync should not issue a LoadQueue (no restart)"
        );
    }

    #[test]
    fn np_002_rescan_while_playing_resync_sends_resync_command_not_loadqueue() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music");
        let t1 = music_dir.join("a.ogg");
        let t2 = music_dir.join("b.ogg");
        write_dummy_file(&t1, 16);
        write_dummy_file(&t2, 16);

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let (mut app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;
        // Use a different path spelling pointing to the same file to exercise the
        // canonicalize fallback in `find_track_index_by_path` (important on Windows).
        let sub = music_dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        app.state.player.current_path = Some(sub.join("..").join("b.ogg"));

        app.apply(Action::RescanLibrary).unwrap();

        match cmd_rx.try_recv().unwrap() {
            PlayerCommand::ResyncQueueAfterLibraryChange {
                tracks,
                current_path,
            } => {
                assert_eq!(tracks.len(), 2);
                // `current_path` must match the exact `PathBuf` we send from the refreshed queue.
                assert_eq!(current_path, tracks[1]);
            }
            other => panic!("expected ResyncQueueAfterLibraryChange, got: {other:?}"),
        }
        assert!(
            cmd_rx.try_recv().is_err(),
            "expected exactly one player command for resync branch"
        );
    }

    #[test]
    fn np_002_rescan_while_playing_missing_current_file_shuffle_off_loads_queue_start_index_0() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music");
        let t1 = music_dir.join("a.ogg");
        let t2 = music_dir.join("b.ogg");
        write_dummy_file(&t1, 16);
        write_dummy_file(&t2, 16);

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.settings.shuffle = false;
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let (mut app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;
        app.state.player.current_path = Some(music_dir.join("missing.ogg"));

        app.apply(Action::RescanLibrary).unwrap();

        match cmd_rx.try_recv().unwrap() {
            PlayerCommand::LoadQueue {
                tracks,
                start_index,
            } => {
                assert_eq!(tracks.len(), 2);
                assert_eq!(start_index, 0);
            }
            other => panic!("expected LoadQueue, got: {other:?}"),
        }
    }

    #[test]
    fn np_002_rescan_while_playing_missing_current_file_reload_uses_seeded_rng_when_shuffled() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music");
        let t1 = music_dir.join("a.ogg");
        let t2 = music_dir.join("b.ogg");
        write_dummy_file(&t1, 16);
        write_dummy_file(&t2, 16);

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.settings.shuffle = true;
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let (mut app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());
        app.set_rng_seed_for_test(123);

        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;
        app.state.player.current_path = Some(music_dir.join("missing.ogg"));

        app.apply(Action::RescanLibrary).unwrap();

        assert_eq!(app.state.library.tracks.len(), 2);
        assert_eq!(
            app.load_queue_commands_sent, 1,
            "fallback must reload queue"
        );
        assert_eq!(
            app.state.status.as_deref(),
            Some("scan complete: queue refreshed")
        );
    }

    #[test]
    fn fix_004_play_rescans_active_folders_then_loads_queue() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music");
        let track = music_dir.join("t1.ogg");
        write_dummy_file(&track, 16);

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let (mut app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());

        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();

        assert_eq!(app.state.library.tracks.len(), 1);
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(app.state.status, None);
    }

    #[test]
    fn idx_001_cached_index_reuse_requires_matching_schema_version() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let manual = ManualScanSpawner::default();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        // Forge a cache that *would* otherwise be reused (tracks non-empty + fingerprint match),
        // but with a schema version mismatch. This must force a rescan.
        let desired_fp = crate::indexer::compute_index_fingerprint(
            &app.state
                .cfg
                .folders
                .iter()
                .map(|f| {
                    let eff_kb = effective_min_size_kb_for_folder(f, &app.state.cfg.settings);
                    let min_size_bytes = eff_kb.saturating_mul(1024);
                    FolderScanEntry {
                        path: f.path.clone(),
                        scan_depth: f.scan_depth,
                        min_size_bytes,
                    }
                })
                .collect::<Vec<_>>(),
            &ScanOptions {
                supported_extensions: app.state.cfg.settings.supported_extensions.clone(),
                min_size_bytes: app.state.cfg.settings.min_size_kb.saturating_mul(1024),
                allow_name_size_fallback_dedup: true,
                force_canonicalize_fail: false,
            },
        );

        let mut lib = LibraryIndex::default();
        lib.schema_version = LibraryIndex::SCHEMA_VERSION.saturating_sub(1);
        lib.index_fingerprint = desired_fp;
        lib.tracks = vec![TrackEntry {
            id: TrackId(1),
            path: std::path::PathBuf::from("A.ogg"),
            rel_path: None,
            size_bytes: 1,
        }];
        app.state.library = lib;

        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();

        assert!(
            app.scan_job.is_some(),
            "schema mismatch must prevent cache reuse and start a scan"
        );
        assert_eq!(
            app.load_queue_commands_sent, 0,
            "must not reuse/load queue from a schema-mismatched cache"
        );
    }

    #[test]
    fn fix_004_play_when_scan_not_running_starts_scan_and_loads_queue_after_completion() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let manual = ManualScanSpawner::default();
        let sender = manual.clone();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        // Trigger Play while no scan is running: should start a scan and NOT load immediately.
        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();
        assert!(app.scan_job.is_some(), "Play should start a scan when idle");
        assert_eq!(app.pending_play_from_library, None);
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.state.status.as_deref(),
            Some("Scanning... (play pending)"),
            "TZ-003: Play should show play-pending status while scan is running"
        );
        assert_eq!(
            app.load_queue_commands_sent, 0,
            "queue must not be loaded until scan completes"
        );

        // The scan job should be tagged with the Play origin so apply_scan_result loads the queue.
        assert_eq!(
            app.scan_job.as_ref().unwrap().origin,
            ScanOrigin::PlayFromLibrary { start_index: 0 }
        );

        // Complete the scan; playback/queue load should happen only after completion is observed.
        let mut lib = LibraryIndex::default();
        lib.tracks = vec![TrackEntry {
            id: TrackId(1),
            path: std::path::PathBuf::from("A.ogg"),
            rel_path: None,
            size_bytes: 1,
        }];
        sender.take_sender().send(lib).unwrap();

        let _ = app.tick().unwrap();

        assert!(app.scan_job.is_none());
        assert_eq!(app.pending_play_from_library, None);
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.load_queue_commands_sent, 1,
            "TZ-003: queue should be loaded exactly once after scan completes"
        );
        assert_eq!(
            app.state.status, None,
            "status should be cleared after queue load"
        );

        // Drive one more tick to ensure we don't "load again" from stale pending state.
        let _ = app.tick().unwrap();
        assert!(app.scan_job.is_none());
        assert_eq!(app.pending_play_from_library, None);
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.load_queue_commands_sent, 1,
            "regression: must not load queue again on subsequent ticks"
        );
    }

    fn lib_with_fingerprint(fp: &str) -> LibraryIndex {
        let mut lib = LibraryIndex::default();
        lib.index_fingerprint = fp.to_string();
        lib
    }

    #[test]
    fn bg_rescan_debounce_does_not_start_scan_before_due() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let manual = ManualScanSpawner::default();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        app.schedule_debounced_background_rescan().unwrap();
        let _ = app.tick().unwrap();
        assert!(
            app.scan_job.is_none(),
            "debounce should prevent immediate scan start"
        );

        app.debounced_scan.as_mut().unwrap().due_at = Instant::now() - Duration::from_millis(1);
        let _ = app.tick().unwrap();
        assert!(
            app.scan_job.is_some(),
            "scan should start once debounce is due"
        );
    }

    #[test]
    fn bg_rescan_rapid_successive_changes_only_latest_token_is_scheduled() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let manual = ManualScanSpawner::default();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        app.schedule_debounced_background_rescan().unwrap();
        let token1 = app.latest_bg_scan_token;
        app.schedule_debounced_background_rescan().unwrap();
        let token2 = app.latest_bg_scan_token;
        assert_ne!(token1, token2);

        let pending = app.debounced_scan.as_ref().unwrap();
        assert_eq!(
            pending.req.origin,
            ScanOrigin::BackgroundRescan { token: token2 },
            "rapid changes should keep only the latest scheduled background rescan"
        );

        app.debounced_scan.as_mut().unwrap().due_at = Instant::now() - Duration::from_millis(1);
        let _ = app.tick().unwrap();

        assert_eq!(
            app.scan_job.as_ref().unwrap().origin,
            ScanOrigin::BackgroundRescan { token: token2 }
        );
    }

    #[test]
    fn bg_rescan_stale_completion_is_ignored_by_token_mismatch_and_latest_is_applied() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let manual = ManualScanSpawner::default();
        let sender = manual.clone();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        // Start background scan #1 (token1).
        app.schedule_debounced_background_rescan().unwrap();
        let token1 = app.latest_bg_scan_token;
        app.debounced_scan.as_mut().unwrap().due_at = Instant::now() - Duration::from_millis(1);
        let _ = app.tick().unwrap();
        assert_eq!(
            app.scan_job.as_ref().unwrap().origin,
            ScanOrigin::BackgroundRescan { token: token1 }
        );
        let tx1 = sender.take_sender();

        // While scan #1 is running, schedule and queue background scan #2 (token2).
        app.schedule_debounced_background_rescan().unwrap();
        let token2 = app.latest_bg_scan_token;
        assert_ne!(token1, token2);
        app.debounced_scan.as_mut().unwrap().due_at = Instant::now() - Duration::from_millis(1);
        let _ = app.tick().unwrap();
        assert!(
            app.pending_scan.is_some(),
            "second background scan should be queued while first is running"
        );

        // Complete scan #1 AFTER token2 is now latest; result must be ignored.
        tx1.send(lib_with_fingerprint("old")).unwrap();
        let _ = app.tick().unwrap();

        // The app should immediately start the queued scan #2.
        assert_eq!(
            app.scan_job.as_ref().unwrap().origin,
            ScanOrigin::BackgroundRescan { token: token2 }
        );
        let tx2 = sender.take_sender();
        tx2.send(lib_with_fingerprint("new")).unwrap();

        let _ = app.tick().unwrap();
        assert_eq!(app.state.library.index_fingerprint, "new");
        assert_ne!(app.state.library.index_fingerprint, "old");
    }

    #[test]
    fn np_001_play_from_main_menu_when_same_folders_source_and_playing_only_navigates() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music_a");
        std::fs::create_dir_all(&music_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let (mut app, mock) = app_with_mock(paths, cfg, PlaylistsFile::default());
        // Replace the real player handle so we can assert no commands are emitted.
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::MainMenu;
        app.state.player.status = PlaybackStatus::Playing;
        app.state.playback_source = Some(PlaybackSource::from_active_playlist_or_folders(
            None,
            &app.state.cfg.folders,
        ));

        let before_cfg_writes = mock.config_writes();
        let before_pls_writes = mock.playlists_writes();
        let before_load_queue_commands_sent = app.load_queue_commands_sent;

        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();

        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(app.state.status, None);
        assert_eq!(mock.config_writes(), before_cfg_writes);
        assert_eq!(mock.playlists_writes(), before_pls_writes);
        assert!(
            cmd_rx.try_recv().is_err(),
            "guard should not emit any PlayerCommand (no Stop/LoadQueue/etc)"
        );
        assert!(
            app.scan_job.is_none() && app.pending_scan.is_none(),
            "guard should not start or queue a scan"
        );
        assert_eq!(
            app.load_queue_commands_sent, before_load_queue_commands_sent,
            "guard should not load/reload queue"
        );
    }

    #[test]
    fn np_001_play_from_main_menu_when_same_playlist_source_and_paused_only_navigates() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music_a");
        std::fs::create_dir_all(&music_dir).unwrap();

        let mut cfg = AppConfig::default();
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let mut pls = PlaylistsFile::default();
        pls.playlists.push(Playlist {
            id: "p1".to_string(),
            name: "A".to_string(),
            folders: vec![FolderEntry::new(music_dir.to_string_lossy().to_string())],
            extra: Default::default(),
        });

        let (mut app, mock) = app_with_mock(paths, cfg, pls);
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.state.screen = Screen::MainMenu;
        app.state.playlists.active = Some("p1".to_string());
        app.state.player.status = PlaybackStatus::Paused;
        app.state.playback_source = Some(PlaybackSource::Playlist("p1".to_string()));

        let before_cfg_writes = mock.config_writes();
        let before_pls_writes = mock.playlists_writes();
        let before_load_queue_commands_sent = app.load_queue_commands_sent;

        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();

        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(app.state.status, None);
        assert_eq!(mock.config_writes(), before_cfg_writes);
        assert_eq!(mock.playlists_writes(), before_pls_writes);
        assert!(
            cmd_rx.try_recv().is_err(),
            "guard should not emit any PlayerCommand (no Stop/LoadQueue/etc)"
        );
        assert!(
            app.scan_job.is_none() && app.pending_scan.is_none(),
            "guard should not start or queue a scan"
        );
        assert_eq!(
            app.load_queue_commands_sent, before_load_queue_commands_sent,
            "guard should not load/reload queue"
        );
    }

    #[test]
    fn np_001_guard_does_not_trigger_when_stopped_even_if_sources_match() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let music_dir = td.path().join("music_a");
        let track = music_dir.join("t1.ogg");
        write_dummy_file(&track, 16);

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new(music_dir.to_string_lossy().to_string())];

        let (mut app, _mock) = app_with_mock(paths, cfg, PlaylistsFile::default());

        app.state.screen = Screen::MainMenu;
        app.state.player.status = PlaybackStatus::Stopped;
        app.state.playback_source = Some(PlaybackSource::from_active_playlist_or_folders(
            None,
            &app.state.cfg.folders,
        ));

        let before_load_queue_commands_sent = app.load_queue_commands_sent;

        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();

        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.state.library.tracks.len(),
            1,
            "stopped playback must still scan/load the library"
        );
        assert!(
            app.load_queue_commands_sent > before_load_queue_commands_sent,
            "stopped playback must load the queue (guard must not early-return)"
        );
    }

    #[test]
    fn np_001_guard_does_not_trigger_when_playing_but_requested_source_differs() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("C:\\MusicA".to_string())];

        let manual = ManualScanSpawner::default();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        app.state.screen = Screen::MainMenu;
        app.state.player.status = PlaybackStatus::Playing;
        // Make the currently-playing source differ from the requested source (which is derived
        // from cfg.folders above) so the NP-001 guard must NOT early-return.
        app.state.playback_source = Some(PlaybackSource::from_active_playlist_or_folders(
            None,
            &[FolderEntry::new("C:\\MusicB".to_string())],
        ));

        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();

        assert!(
            app.scan_job.is_some(),
            "different requested source must start a scan (guard must not early-return)"
        );
        assert_eq!(
            app.scan_job.as_ref().unwrap().origin,
            ScanOrigin::PlayFromLibrary { start_index: 0 }
        );
        assert_eq!(app.state.screen, Screen::NowPlaying);
    }

    #[test]
    fn fix_004_play_while_scan_running_sets_pending_and_loads_after_completion() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let manual = ManualScanSpawner::default();
        let sender = manual.clone();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        // Start a scan that does not complete yet.
        app.apply(Action::RescanLibrary).unwrap();
        assert!(app.scan_job.is_some());
        let scan_started_at = app.scan_job.as_ref().unwrap().started_at;
        assert_eq!(
            app.scan_job.as_ref().unwrap().origin,
            ScanOrigin::RescanLibrary,
            "rescan action should tag scan origin as RescanLibrary"
        );

        // Request play during scan; should not start another scan.
        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();
        assert!(app.scan_job.is_some());
        assert_eq!(
            app.scan_job.as_ref().unwrap().started_at,
            scan_started_at,
            "Play during scan must not start a second scan"
        );
        assert_eq!(
            app.scan_job.as_ref().unwrap().origin,
            ScanOrigin::RescanLibrary,
            "Play during scan must not replace the active scan origin"
        );
        assert_eq!(app.pending_play_from_library, Some(0));
        assert_eq!(
            app.state.screen,
            Screen::NowPlaying,
            "TZ-003: should navigate to NowPlaying immediately even if scan already running"
        );
        assert_eq!(
            app.state.status.as_deref(),
            Some("Scanning... (play pending)"),
            "TZ-003: status must include '(play pending)' when Play is pressed during an active scan"
        );
        assert_eq!(
            app.load_queue_commands_sent, 0,
            "queue must not be loaded until scan completes"
        );

        // Complete the scan.
        let mut lib = LibraryIndex::default();
        lib.tracks = vec![TrackEntry {
            id: TrackId(1),
            path: std::path::PathBuf::from("A.ogg"),
            rel_path: None,
            size_bytes: 1,
        }];
        sender.take_sender().send(lib).unwrap();

        // Drive the app loop so completion is observed and play pending is applied.
        let _ = app.tick().unwrap();

        assert_eq!(app.pending_play_from_library, None);
        assert!(app.scan_job.is_none());
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.load_queue_commands_sent, 1,
            "TZ-003: queue should be loaded after scan completes"
        );
        assert_eq!(
            app.state.status, None,
            "status should be cleared after queue load"
        );

        // And ensure it does not re-apply on subsequent ticks.
        let _ = app.tick().unwrap();
        assert_eq!(app.pending_play_from_library, None);
        assert!(app.scan_job.is_none());
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.load_queue_commands_sent, 1,
            "regression: must not load queue again on subsequent ticks"
        );
    }

    #[test]
    fn fix_004_rescan_completion_with_pending_play_issues_only_one_load_queue() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());

        let mut cfg = AppConfig::default();
        cfg.settings.min_size_kb = 0;
        cfg.settings.min_size_bytes = 0;
        cfg.folders = vec![FolderEntry::new("C:\\Music".to_string())];

        let manual = ManualScanSpawner::default();
        let sender = manual.clone();
        let (mut app, _mock) =
            app_with_mock_and_scan_spawner(paths, cfg, PlaylistsFile::default(), Box::new(manual));

        // Start a rescan that does not complete yet.
        app.apply(Action::RescanLibrary).unwrap();
        assert!(app.scan_job.is_some());

        // Simulate "playing" state during the scan.
        app.state.screen = Screen::NowPlaying;
        app.state.player.status = PlaybackStatus::Playing;
        app.state.player.current_path = Some(std::path::PathBuf::from("B.ogg"));

        // User hits Play during the scan -> pending_play_from_library is set.
        app.apply(Action::PlayerLoadFromLibrary { start_index: 0 })
            .unwrap();
        assert_eq!(app.pending_play_from_library, Some(0));
        assert_eq!(
            app.state.status.as_deref(),
            Some("Scanning... (play pending)"),
            "queued-scan status must not overwrite play-pending status"
        );

        // Complete the scan with a non-empty library.
        let mut lib = LibraryIndex::default();
        lib.tracks = vec![
            TrackEntry {
                id: TrackId(1),
                path: std::path::PathBuf::from("A.ogg"),
                rel_path: None,
                size_bytes: 1,
            },
            TrackEntry {
                id: TrackId(2),
                path: std::path::PathBuf::from("B.ogg"),
                rel_path: None,
                size_bytes: 1,
            },
        ];
        sender.take_sender().send(lib).unwrap();

        // Observe completion.
        let _ = app.tick().unwrap();

        assert!(app.scan_job.is_none());
        assert_eq!(app.pending_play_from_library, None);
        assert_eq!(app.state.screen, Screen::NowPlaying);

        assert_eq!(
            app.load_queue_commands_sent, 1,
            "rescan completion + pending play must issue exactly one LoadQueue"
        );
    }

    #[test]
    fn fix_006_last_error_propagates_and_clears_from_player_snapshots() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        // Replace the real player handle with a controllable one.
        let (cmd_tx, _cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        // First snapshot carries an error.
        evt_tx
            .send(PlayerEvent::Snapshot(crate::player::PlayerSnapshot {
                status: PlaybackStatus::Playing,
                current_path: Some(std::path::PathBuf::from("bad.ogg")),
                shuffle: false,
                repeat: RepeatMode::Off,
                volume_percent: 50,
                queue_pos: Some(0),
                queue_len: 2,
                track_position: std::time::Duration::from_secs(0),
                track_duration: None,
                last_error: Some("decode failed: bad.ogg".to_string()),
            }))
            .unwrap();

        app.drain_player_events();

        assert_eq!(
            app.state.last_error.as_deref(),
            Some("decode failed: bad.ogg")
        );
        assert_eq!(
            app.state.player.last_error.as_deref(),
            Some("decode failed: bad.ogg")
        );

        // Next snapshot may clear last_error; TUI should clear state.last_error as well.
        evt_tx
            .send(PlayerEvent::Snapshot(crate::player::PlayerSnapshot {
                status: PlaybackStatus::Playing,
                current_path: Some(std::path::PathBuf::from("good.ogg")),
                shuffle: false,
                repeat: RepeatMode::Off,
                volume_percent: 50,
                queue_pos: Some(1),
                queue_len: 2,
                track_position: std::time::Duration::from_secs(1),
                track_duration: None,
                last_error: None,
            }))
            .unwrap();

        app.drain_player_events();

        assert_eq!(
            app.state.last_error.as_deref(),
            None,
            "last_error should clear in AppState when snapshots have None"
        );
        assert_eq!(
            app.state.player.current_path.as_deref(),
            Some(std::path::Path::new("good.ogg"))
        );
    }

    #[test]
    fn tz_005_focus_activity_action_is_forwarded_to_player_as_set_ui_activity_command() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let cfg = AppConfig::default();
        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        // Replace the real player handle with a controllable one.
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.apply(Action::PlayerSetUiActivity {
            focused: false,
            minimized: true,
        })
        .unwrap();

        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::SetUiActivity {
                focused: false,
                minimized: true
            }
        );
    }

    #[test]
    fn volume_actions_send_set_volume_percent_to_prev_next_rung() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.audio.volume_available_percent = vec![0u8, 5, 7, 10, 100];
        cfg.audio.volume_default_percent = 7;
        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        // Replace the real player handle with a controllable one.
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.apply(Action::VolumeUp).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::SetVolumePercent(10)
        );

        app.apply(Action::VolumeDown).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::SetVolumePercent(5)
        );
    }

    #[test]
    fn volume_actions_use_prev_next_even_when_current_volume_is_not_exactly_on_a_rung() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.audio.volume_available_percent = vec![0u8, 5, 7, 10, 100];
        cfg.audio.volume_default_percent = 6;
        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.apply(Action::VolumeUp).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::SetVolumePercent(7)
        );

        app.apply(Action::VolumeDown).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::SetVolumePercent(5)
        );
    }
}
