use crate::config::{self, FolderEntry, RepeatMode};
use crate::error::{AppError, AppResult};
use crate::indexer::{self, FolderScanEntry, ScanOptions};
use crate::player::{PlayerCommand, PlayerEvent, PlayerHandle};
use crate::playlists::{self, Playlist};
use crate::state as app_state_file;
use crate::tui::action::{Action, Screen};
use crate::tui::screens::{MainMenuScreen, NowPlayingScreen, PlaylistsScreen, SettingsScreen};
use crate::tui::state::AppState;
use crate::{config::AppConfig, paths::AppPaths, playlists::PlaylistsFile};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
#[cfg(test)]
use std::time::Instant;

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
    scan_spawner: Box<dyn ScanSpawner>,
    scan_job: Option<ScanJobState>,
    pending_scan: Option<ScanRequest>,
    pending_play_from_library: Option<usize>,
    #[cfg(test)]
    load_queue_commands_sent: usize,
}

fn library_tracks_to_paths(library: &crate::indexer::LibraryIndex) -> Vec<std::path::PathBuf> {
    library.tracks.iter().map(|t| t.path.clone()).collect()
}

fn find_track_index_by_path(tracks: &[PathBuf], needle: &PathBuf) -> Option<usize> {
    tracks.iter().position(|p| p == needle)
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
            cfg.audio.default_volume_percent,
        );
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
            scan_spawner,
            scan_job: None,
            pending_scan: None,
            pending_play_from_library: None,
            #[cfg(test)]
            load_queue_commands_sent: 0,
        }
    }

    pub fn tick(&mut self) -> AppResult<Option<Action>> {
        self.drain_player_events();
        self.poll_scan_job_completion();
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
                self.state.status = Some("folder added and saved".to_string());
            }
            Action::RemoveFolderAt(idx) => {
                if idx >= self.state.cfg.folders.len() {
                    return Ok(());
                }
                self.state.cfg.folders.remove(idx);
                self.state.cfg = self.state.cfg.clone().normalized();
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                if self.state.main_selected_folder >= self.state.cfg.folders.len() {
                    self.state.main_selected_folder =
                        self.state.cfg.folders.len().saturating_sub(1);
                }
                self.state.status = Some("folder removed and saved".to_string());
            }
            Action::ToggleFolderRootOnlyAt(idx) => {
                let Some(folder) = self.state.cfg.folders.get_mut(idx) else {
                    return Ok(());
                };
                let new_value = !folder.root_only;
                folder.root_only = new_value;
                self.state.cfg = self.state.cfg.clone().normalized();
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
                let label = if new_value { "on" } else { "off" };
                self.state.status = Some(format!("root_only toggled: {label}"));
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
                self.state.status = Some("settings saved".to_string());
            }
            Action::ToggleShuffle => {
                self.state.cfg.settings.shuffle = !self.state.cfg.settings.shuffle;
                self.persistence
                    .save_config(&self.state.paths, &self.state.cfg)?;
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

                // FIX-004: Always rescan ACTIVE folders before building the queue, to avoid
                // enqueueing stale tracks after folder changes.
                if self.scan_job.is_some() {
                    // Do not start a second scan; mark play pending and complete it right after
                    // the current scan finishes.
                    self.pending_play_from_library = Some(start_index);
                    self.state.status = Some("Scanning... (play pending)".to_string());
                    return Ok(());
                }

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
                let step = self.state.cfg.audio.volume_step_percent.min(100) as i8;
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::AdjustVolumePercent(step));
                }
            }
            Action::VolumeDown => {
                let step = self.state.cfg.audio.volume_step_percent.min(100) as i8;
                if let Some(p) = self.player.as_ref() {
                    p.send(PlayerCommand::AdjustVolumePercent(-step));
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
                self.persistence
                    .save_playlists(&self.state.paths, &self.state.playlists)?;
                self.state.status = Some("playlist created and saved".to_string());
            }
            Action::RenamePlaylist { idx, name } => {
                let name = name.trim();
                if name.is_empty() {
                    self.state.status = Some("playlist name must not be empty".to_string());
                    return Ok(());
                }
                if let Some(p) = self.state.playlists.playlists.get_mut(idx) {
                    p.name = name.to_string();
                    p.validate()
                        .map_err(|msg| AppError::Config { message: msg })?;
                    self.persistence
                        .save_playlists(&self.state.paths, &self.state.playlists)?;
                    self.state.status = Some("playlist renamed and saved".to_string());
                }
            }
            Action::DeletePlaylist { idx } => {
                if idx >= self.state.playlists.playlists.len() {
                    return Ok(());
                }
                let deleted_id = self.state.playlists.playlists[idx].id.clone();
                self.state.playlists.playlists.remove(idx);
                if self.state.playlists.active.as_deref() == Some(deleted_id.as_str()) {
                    self.state.playlists.active = None;
                }
                self.persistence
                    .save_playlists(&self.state.paths, &self.state.playlists)?;
                self.state.status = Some("playlist deleted and saved".to_string());
                if self.state.playlists_selected >= self.state.playlists.playlists.len() {
                    self.state.playlists_selected =
                        self.state.playlists.playlists.len().saturating_sub(1);
                }
            }
            Action::OverwritePlaylistWithCurrent { idx } => {
                if let Some(p) = self.state.playlists.playlists.get_mut(idx) {
                    p.folders = self.state.cfg.folders.clone();
                    self.persistence
                        .save_playlists(&self.state.paths, &self.state.playlists)?;
                    self.state.status = Some("playlist overwritten and saved".to_string());
                }
            }
            Action::LoadPlaylist { idx } => {
                if let Some(p) = self.state.playlists.playlists.get(idx) {
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
                    self.persistence
                        .save_playlists(&self.state.paths, &self.state.playlists)?;

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
        let min_size_bytes = self
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
            .map(|f| FolderScanEntry {
                path: f.path.clone(),
                root_only: f.root_only,
            })
            .collect::<Vec<_>>();
        Ok(ScanRequest {
            folders,
            opts: ScanOptions {
                supported_extensions: self.state.cfg.settings.supported_extensions.clone(),
                min_size_bytes,
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

        self.state.status = Some("Scanning...".to_string());
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
            tracing::warn!(error = %e, "failed to persist index cache");
        }
        // Best-effort runtime state tracking under data/state.yaml.
        if let Ok(mut st) = app_state_file::load_or_create(&self.state.paths) {
            st.last_index = Some(app_state_file::LastIndexSummary {
                tracks_total: tracks,
                issues_total: issues,
            });
            if let Err(e) = app_state_file::save(&self.state.paths, &st) {
                tracing::warn!(error = %e, "failed to persist state.yaml");
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

                    let start_index = self
                        .state
                        .player
                        .current_path
                        .as_ref()
                        .and_then(|cur| find_track_index_by_path(&new_tracks, cur))
                        .unwrap_or(0);
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
                    self.state.status = Some(
                        "scan complete: queue refreshed (restarted current track)".to_string(),
                    );
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
            playlists_dir: data_dir.join("playlists"),
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
            root_only: true,
        }];
        let pls = PlaylistsFile::default();
        let (mut app, mock) = app_with_mock(paths, cfg, pls);

        app.apply(Action::ToggleFolderRootOnlyAt(0)).unwrap();

        assert_eq!(mock.config_writes(), 1);
        assert_eq!(
            app.state.cfg.folders,
            vec![FolderEntry {
                path: "C:\\Music".to_string(),
                root_only: false
            }]
        );
        assert_eq!(app.state.status.as_deref(), Some("root_only toggled: off"));
    }

    #[test]
    fn toggle_folder_root_only_updates_only_selected_index_and_saves() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.folders = vec![
            FolderEntry {
                path: "A".to_string(),
                root_only: true,
            },
            FolderEntry {
                path: "B".to_string(),
                root_only: false,
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
                    root_only: true,
                },
                FolderEntry {
                    path: "B".to_string(),
                    root_only: true,
                }
            ]
        );
        assert_eq!(app.state.status.as_deref(), Some("root_only toggled: on"));
    }

    #[test]
    fn create_playlist_saves_playlists_and_copies_current_folders() {
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

        assert_eq!(mock.playlists_writes(), 1);
        assert_eq!(app.state.playlists.playlists.len(), 1);
        assert_eq!(app.state.playlists.playlists[0].name, "My Mix");
        assert_eq!(
            app.state.playlists.playlists[0].folders,
            vec![
                FolderEntry::new("A".to_string()),
                FolderEntry::new("B".to_string())
            ]
        );
        assert_eq!(
            mock.last_saved_playlists
                .borrow()
                .as_ref()
                .unwrap()
                .playlists
                .len(),
            1
        );
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist created and saved")
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
        assert_eq!(mock.playlists_writes(), 1);
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist renamed and saved")
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
    fn delete_playlist_clears_active_and_saves() {
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

        assert_eq!(mock.playlists_writes(), 1);
        assert_eq!(app.state.playlists.playlists.len(), 1);
        assert_eq!(app.state.playlists.active, None);
        assert_eq!(
            app.state.status.as_deref(),
            Some("playlist deleted and saved")
        );
        assert_eq!(app.state.playlists_selected, 0);
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
    fn load_playlist_swaps_folders_sets_active_and_saves_both() {
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
        assert_eq!(mock.playlists_writes(), 1);
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

        app.apply(Action::RescanLibrary).unwrap();

        assert_eq!(app.state.library.tracks.len(), 2);
        assert_eq!(app.state.screen, Screen::NowPlaying);
        assert_eq!(
            app.state.status.as_deref(),
            Some("scan complete: queue refreshed (restarted current track)")
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
        assert_eq!(app.state.screen, Screen::MainMenu);

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

        // Drive one more tick to ensure we don't "load again" from stale pending state.
        let _ = app.tick().unwrap();
        assert!(app.scan_job.is_none());
        assert_eq!(app.pending_play_from_library, None);
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

        // And ensure it does not re-apply on subsequent ticks.
        let _ = app.tick().unwrap();
        assert_eq!(app.pending_play_from_library, None);
        assert!(app.scan_job.is_none());
        assert_eq!(app.state.screen, Screen::NowPlaying);
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
    fn volume_actions_send_adjust_volume_command_with_config_step_and_sign() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.audio.volume_step_percent = 7;
        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        // Replace the real player handle with a controllable one.
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.apply(Action::VolumeUp).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::AdjustVolumePercent(7)
        );

        app.apply(Action::VolumeDown).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::AdjustVolumePercent(-7)
        );
    }

    #[test]
    fn volume_actions_cap_step_to_100_before_sending() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        let mut cfg = AppConfig::default();
        cfg.audio.volume_step_percent = 250;
        let pls = PlaylistsFile::default();
        let (mut app, _mock) = app_with_mock(paths, cfg, pls);

        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (_evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();
        app.player = Some(PlayerHandle::new_for_test(cmd_tx, evt_rx));

        app.apply(Action::VolumeUp).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::AdjustVolumePercent(100)
        );

        app.apply(Action::VolumeDown).unwrap();
        assert_eq!(
            cmd_rx.try_recv().unwrap(),
            PlayerCommand::AdjustVolumePercent(-100)
        );
    }
}
