use crate::config::RepeatMode;
use queue::PlayerQueue;
use rodio::Source;
use std::any::Any;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub mod queue;

trait AudioSinkLike {
    fn play(&self);
    fn pause(&self);
    fn stop(&self);
    fn empty(&self) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

trait AudioBackend {
    fn try_init(&mut self) -> Result<(), String>;
    fn create_sink(&self) -> Result<Box<dyn AudioSinkLike>, String>;
    fn append_file(&self, sink: &mut dyn AudioSinkLike, path: &std::path::Path)
        -> Result<Option<Duration>, String>;
}

#[derive(Default)]
struct RodioBackend {
    stream: Option<rodio::OutputStream>,
    stream_handle: Option<rodio::OutputStreamHandle>,
}

impl RodioBackend {
    fn new() -> Self {
        Self::default()
    }
}

struct RodioSink(rodio::Sink);

impl AudioSinkLike for RodioSink {
    fn play(&self) {
        self.0.play();
    }
    fn pause(&self) {
        self.0.pause();
    }
    fn stop(&self) {
        self.0.stop();
    }
    fn empty(&self) -> bool {
        self.0.empty()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl AudioBackend for RodioBackend {
    fn try_init(&mut self) -> Result<(), String> {
        let (stream, stream_handle) = crate::audio::try_default_output()?;
        self.stream = Some(stream);
        self.stream_handle = Some(stream_handle);
        Ok(())
    }

    fn create_sink(&self) -> Result<Box<dyn AudioSinkLike>, String> {
        let Some(stream_handle) = &self.stream_handle else {
            return Err("audio output unavailable (no default output device?)".to_string());
        };
        let sink = rodio::Sink::try_new(stream_handle)
            .map_err(|e| format!("failed to create audio sink: {e}"))?;
        Ok(Box::new(RodioSink(sink)))
    }

    fn append_file(
        &self,
        sink: &mut dyn AudioSinkLike,
        path: &std::path::Path,
    ) -> Result<Option<Duration>, String> {
        let Some(rodio_sink) = sink.as_any_mut().downcast_mut::<RodioSink>() else {
            return Err("internal error: sink/backend mismatch".to_string());
        };
        let source = crate::audio::decode_file(path)?;
        let duration = source.total_duration();
        rodio_sink.0.append(source);
        Ok(duration)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    pub status: PlaybackStatus,
    pub current_path: Option<PathBuf>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub queue_pos: Option<usize>,
    pub queue_len: usize,
    pub track_position: Duration,
    pub track_duration: Option<Duration>,
}

impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            current_path: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            queue_pos: None,
            queue_len: 0,
            track_position: Duration::from_secs(0),
            track_duration: None,
        }
    }
}

#[derive(Debug)]
pub enum PlayerCommand {
    LoadQueue {
        tracks: Vec<PathBuf>,
        start_index: usize,
    },
    TogglePlayPause,
    Stop,
    Next,
    Prev,
    SeekRelativeSeconds(i64),
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Snapshot(PlayerSnapshot),
    Error(String),
    ShutdownAck,
}

pub struct PlayerHandle {
    cmd_tx: mpsc::Sender<PlayerCommand>,
    evt_rx: mpsc::Receiver<PlayerEvent>,
    join: Option<thread::JoinHandle<()>>,
}

impl PlayerHandle {
    pub fn spawn(initial_shuffle: bool, initial_repeat: RepeatMode) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (evt_tx, evt_rx) = mpsc::channel::<PlayerEvent>();

        let join = thread::spawn(move || playback_thread(cmd_rx, evt_tx, initial_shuffle, initial_repeat));

        Self {
            cmd_tx,
            evt_rx,
            join: Some(join),
        }
    }

    pub fn send(&self, cmd: PlayerCommand) {
        // Best-effort. If UI is shutting down, ignore send failures.
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<PlayerEvent> {
        self.evt_rx.try_recv().ok()
    }

    pub fn shutdown(&self) {
        self.send(PlayerCommand::Shutdown);
    }

    pub fn join(mut self) -> Result<(), String> {
        let Some(j) = self.join.take() else {
            return Ok(());
        };
        j.join()
            .map_err(|_| "player thread panicked".to_string())
    }

    pub fn shutdown_and_join(mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;

        // Ask the playback thread to stop.
        self.shutdown();

        // Drain events until we see ShutdownAck (ignore snapshots/errors).
        // This avoids being "thrown off" by queued Snapshot/Error events.
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err("timed out waiting for player shutdown".to_string());
            }
            let remaining = deadline.saturating_duration_since(now);
            match self.evt_rx.recv_timeout(remaining) {
                Ok(PlayerEvent::ShutdownAck) => break,
                Ok(_other) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err("timed out waiting for player shutdown".to_string());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        // Join with whatever time remains.
        let now = Instant::now();
        if now >= deadline {
            return Err("timed out joining player thread".to_string());
        }
        let remaining = deadline.saturating_duration_since(now);
        self.join_with_timeout(remaining)
    }

    fn join_with_timeout(mut self, timeout: Duration) -> Result<(), String> {
        let Some(j) = self.join.take() else {
            return Ok(());
        };

        let (done_tx, done_rx) = mpsc::channel::<Result<(), String>>();
        thread::spawn(move || {
            let res = j
                .join()
                .map_err(|_| "player thread panicked".to_string());
            let _ = done_tx.send(res);
        });

        match done_rx.recv_timeout(timeout) {
            Ok(res) => res,
            Err(mpsc::RecvTimeoutError::Timeout) => Err("timed out joining player thread".to_string()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("join waiter disconnected unexpectedly".to_string())
            }
        }
    }
}

fn playback_thread(
    cmd_rx: mpsc::Receiver<PlayerCommand>,
    evt_tx: mpsc::Sender<PlayerEvent>,
    initial_shuffle: bool,
    initial_repeat: RepeatMode,
) {
    let mut engine = Engine::new(initial_shuffle, initial_repeat);
    engine.emit_snapshot(&evt_tx);

    let tick = Duration::from_millis(100);
    loop {
        match cmd_rx.recv_timeout(tick) {
            Ok(cmd) => {
                if let PlayerCommand::Shutdown = cmd {
                    engine.stop();
                    let _ = evt_tx.send(PlayerEvent::ShutdownAck);
                    break;
                }
                if let Err(msg) = engine.on_command(cmd) {
                    let _ = evt_tx.send(PlayerEvent::Error(msg));
                }
                engine.emit_snapshot(&evt_tx);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if engine.on_tick() {
                    engine.emit_snapshot(&evt_tx);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                engine.stop();
                break;
            }
        }
    }
}

struct Engine {
    backend: Box<dyn AudioBackend>,
    backend_available: bool,
    sink: Option<Box<dyn AudioSinkLike>>,

    queue: PlayerQueue,

    status: PlaybackStatus,
    shuffle: bool,
    repeat: RepeatMode,
    current_path: Option<PathBuf>,
    current_duration: Option<Duration>,
}

impl Engine {
    fn new(shuffle: bool, repeat: RepeatMode) -> Self {
        let mut backend = RodioBackend::new();
        let backend_available = backend.try_init().is_ok();
        Self {
            backend: Box::new(backend),
            backend_available,
            sink: None,
            queue: PlayerQueue::default(),
            status: PlaybackStatus::Stopped,
            shuffle,
            repeat,
            current_path: None,
            current_duration: None,
        }
    }

    #[cfg(test)]
    fn new_with_backend(shuffle: bool, repeat: RepeatMode, backend: Box<dyn AudioBackend>) -> Self {
        let mut backend = backend;
        let backend_available = backend.try_init().is_ok();
        Self {
            backend,
            backend_available,
            sink: None,
            queue: PlayerQueue::default(),
            status: PlaybackStatus::Stopped,
            shuffle,
            repeat,
            current_path: None,
            current_duration: None,
        }
    }

    fn emit_snapshot(&self, evt_tx: &mpsc::Sender<PlayerEvent>) {
        let queue_pos = self.queue.pos_in_order();
        let queue_len = self.queue.order_len();
        let track_position = self.current_sink_pos().unwrap_or_else(|| Duration::from_secs(0));
        let _ = evt_tx.send(PlayerEvent::Snapshot(PlayerSnapshot {
            status: self.status,
            current_path: self.current_path.clone(),
            shuffle: self.shuffle,
            repeat: self.repeat,
            queue_pos,
            queue_len,
            track_position,
            track_duration: self.current_duration,
        }));
    }

    fn on_tick(&mut self) -> bool {
        if self.status != PlaybackStatus::Playing {
            return false;
        }
        let Some(sink) = self.sink.as_ref() else {
            return false;
        };
        if !sink.empty() {
            return false;
        }
        // Track finished.
        self.advance_after_end()
    }

    fn on_command(&mut self, cmd: PlayerCommand) -> Result<(), String> {
        match cmd {
            PlayerCommand::LoadQueue { tracks, start_index } => {
                self.queue.load(tracks, start_index, self.shuffle)?;
                if self.queue.is_empty() {
                    self.stop();
                    return Ok(());
                }
                let start_pos = self.queue.pos_in_order().unwrap_or(0);
                self.play_at_pos(start_pos)
            }
            PlayerCommand::TogglePlayPause => self.toggle_play_pause(),
            PlayerCommand::Stop => {
                self.stop();
                Ok(())
            }
            PlayerCommand::Next => self.next(),
            PlayerCommand::Prev => self.prev(),
            PlayerCommand::SeekRelativeSeconds(delta) => self.seek_relative(delta),
            PlayerCommand::SetShuffle(v) => {
                self.shuffle = v;
                self.queue.set_shuffle(self.shuffle);
                Ok(())
            }
            PlayerCommand::SetRepeat(v) => {
                self.repeat = v;
                Ok(())
            }
            PlayerCommand::Shutdown => Ok(()),
        }
    }

    fn current_track_index(&self) -> Option<usize> {
        self.queue.current_index()
    }

    fn toggle_play_pause(&mut self) -> Result<(), String> {
        match self.status {
            PlaybackStatus::Stopped => {
                // If we have a queue and a current position, start it. Otherwise, no-op.
                let Some(pos) =
                    self.queue
                        .pos_in_order()
                        .or_else(|| (!self.queue.is_empty()).then_some(0))
                else {
                    return Ok(());
                };
                self.play_at_pos(pos)
            }
            PlaybackStatus::Playing => {
                self.with_sink(|s| s.pause());
                self.status = PlaybackStatus::Paused;
                Ok(())
            }
            PlaybackStatus::Paused => {
                self.with_sink(|s| s.play());
                self.status = PlaybackStatus::Playing;
                Ok(())
            }
        }
    }

    fn stop(&mut self) {
        self.with_sink(|s| s.stop());
        self.sink = None;
        self.status = PlaybackStatus::Stopped;
        self.current_path = None;
        self.current_duration = None;
    }

    fn next(&mut self) -> Result<(), String> {
        if self.queue.is_empty() {
            return Ok(());
        }
        let next_pos = match self.queue.pos_in_order() {
            None => 0,
            Some(pos) => {
                let is_last = pos + 1 >= self.queue.order_len();
                if is_last {
                    match self.repeat {
                        RepeatMode::All => 0,
                        RepeatMode::Off | RepeatMode::One => {
                            self.stop();
                            return Ok(());
                        }
                    }
                } else {
                    pos + 1
                }
            }
        };
        self.play_at_pos(next_pos)
    }

    fn prev(&mut self) -> Result<(), String> {
        if self.queue.is_empty() {
            return Ok(());
        }
        let prev_pos = match self.queue.pos_in_order() {
            None => 0,
            Some(0) => 0,
            Some(pos) => pos - 1,
        };
        self.play_at_pos(prev_pos)
    }

    fn advance_after_end(&mut self) -> bool {
        match self.repeat {
            RepeatMode::One => {
                if let Some(pos) = self.queue.pos_in_order() {
                    if self.play_at_pos(pos).is_ok() {
                        return true;
                    }
                }
                self.stop();
                true
            }
            RepeatMode::Off | RepeatMode::All => {
                if self.next().is_ok() {
                    return true;
                }
                false
            }
        }
    }

    fn play_at_pos(&mut self, pos_in_order: usize) -> Result<(), String> {
        let path = self
            .queue
            .path_at_pos_in_order(pos_in_order)
            .ok_or_else(|| "track index out of range".to_string())?;

        if !self.backend_available {
            return Err("audio output unavailable (no default output device?)".to_string());
        }

        // Build everything first; only commit state after success.
        let mut new_sink = self.backend.create_sink()?;
        let duration = self.backend.append_file(new_sink.as_mut(), &path)?;
        new_sink.play();

        self.queue.set_pos_in_order(pos_in_order)?;
        if let Some(old) = self.sink.as_ref() {
            old.stop();
        }
        self.sink = Some(new_sink);
        self.status = PlaybackStatus::Playing;
        self.current_path = Some(path);
        self.current_duration = duration;
        Ok(())
    }

    fn with_sink(&self, f: impl FnOnce(&dyn AudioSinkLike)) {
        if let Some(s) = self.sink.as_ref() {
            f(s.as_ref());
        }
    }

    fn seek_relative(&mut self, delta_seconds: i64) -> Result<(), String> {
        if delta_seconds == 0 {
            return Ok(());
        }
        let Some(sink) = self.sink.as_mut() else {
            return Ok(());
        };
        let Some(rodio_sink) = sink.as_any_mut().downcast_mut::<RodioSink>() else {
            return Err("internal error: sink/backend mismatch".to_string());
        };

        let pos = rodio_sink.0.get_pos();
        let delta = if delta_seconds.is_negative() {
            Duration::from_secs(delta_seconds.unsigned_abs())
        } else {
            Duration::from_secs(delta_seconds as u64)
        };
        let target = if delta_seconds.is_negative() {
            pos.saturating_sub(delta)
        } else {
            pos.saturating_add(delta)
        };

        rodio_sink
            .0
            .try_seek(target)
            .map_err(|e| format!("seek not available for this track: {e}"))?;
        Ok(())
    }

    fn current_sink_pos(&self) -> Option<Duration> {
        let sink = self.sink.as_ref()?;
        let rodio_sink = sink.as_ref().as_any().downcast_ref::<RodioSink>()?;
        Some(rodio_sink.0.get_pos())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone)]
    struct MockBackend {
        sink_empty: std::sync::Arc<std::sync::atomic::AtomicBool>,
        append_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        default_output_ok: bool,
        fail_append: bool,
    }

    #[derive(Default)]
    struct MockSink {
        empty: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl AudioSinkLike for MockSink {
        fn play(&self) {}
        fn pause(&self) {}
        fn stop(&self) {}
        fn empty(&self) -> bool {
            self.empty.load(std::sync::atomic::Ordering::Relaxed)
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    impl AudioBackend for MockBackend {
        fn try_init(&mut self) -> Result<(), String> {
            if self.default_output_ok {
                Ok(())
            } else {
                Err("no output".to_string())
            }
        }

        fn create_sink(&self) -> Result<Box<dyn AudioSinkLike>, String> {
            Ok(Box::new(MockSink {
                empty: self.sink_empty.clone(),
            }))
        }

        fn append_file(
            &self,
            _sink: &mut dyn AudioSinkLike,
            _path: &std::path::Path,
        ) -> Result<Option<Duration>, String> {
            if self.fail_append {
                return Err("decode failed".to_string());
            }
            self.append_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(None)
        }
    }

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    fn backend_ok() -> MockBackend {
        MockBackend {
            sink_empty: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            append_calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            default_output_ok: true,
            fail_append: false,
        }
    }

    #[test]
    fn play_pause_play_transitions_do_not_require_real_audio() {
        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg"), p("b.ogg")],
            start_index: 0,
        })
        .unwrap();
        assert_eq!(e.status, PlaybackStatus::Playing);

        e.on_command(PlayerCommand::TogglePlayPause).unwrap();
        assert_eq!(e.status, PlaybackStatus::Paused);

        e.on_command(PlayerCommand::TogglePlayPause).unwrap();
        assert_eq!(e.status, PlaybackStatus::Playing);
    }

    #[test]
    fn stop_clears_current_path_and_status() {
        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg")],
            start_index: 0,
        })
        .unwrap();
        assert_eq!(e.status, PlaybackStatus::Playing);
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));

        e.on_command(PlayerCommand::Stop).unwrap();
        assert_eq!(e.status, PlaybackStatus::Stopped);
        assert!(e.current_path.is_none());
    }

    #[test]
    fn next_at_end_respects_repeat_all_vs_off() {
        let tracks = vec![p("a.ogg"), p("b.ogg")];

        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: tracks.clone(),
            start_index: 1,
        })
        .unwrap();
        e.on_command(PlayerCommand::Next).unwrap();
        assert_eq!(e.status, PlaybackStatus::Stopped);
        assert!(e.current_path.is_none());

        let mut e = Engine::new_with_backend(false, RepeatMode::All, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks,
            start_index: 1,
        })
        .unwrap();
        e.on_command(PlayerCommand::Next).unwrap();
        assert_eq!(e.status, PlaybackStatus::Playing);
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));
    }

    #[test]
    fn repeat_one_restarts_same_track_on_tick_end() {
        let mut e = Engine::new_with_backend(false, RepeatMode::One, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg"), p("b.ogg")],
            start_index: 0,
        })
        .unwrap();
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));

        // We can call the deterministic end-of-track transition directly without
        // depending on real audio output / sink state.
        let changed = e.advance_after_end();
        assert!(changed);
        assert_eq!(e.status, PlaybackStatus::Playing);
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));
    }

    #[test]
    fn toggle_play_pause_when_stopped_with_no_queue_is_noop() {
        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(backend_ok()));
        assert_eq!(e.status, PlaybackStatus::Stopped);
        e.on_command(PlayerCommand::TogglePlayPause).unwrap();
        assert_eq!(e.status, PlaybackStatus::Stopped);
        assert!(e.current_path.is_none());
    }

    #[test]
    fn prev_at_start_stays_on_first_track() {
        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg"), p("b.ogg")],
            start_index: 0,
        })
        .unwrap();
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));

        e.on_command(PlayerCommand::Prev).unwrap();
        assert_eq!(e.status, PlaybackStatus::Playing);
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));
    }

    #[test]
    fn next_at_end_with_repeat_one_stops() {
        let mut e = Engine::new_with_backend(false, RepeatMode::One, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg"), p("b.ogg")],
            start_index: 1,
        })
        .unwrap();
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("b.ogg")));

        e.on_command(PlayerCommand::Next).unwrap();
        assert_eq!(e.status, PlaybackStatus::Stopped);
        assert!(e.current_path.is_none());
    }

    #[test]
    fn advance_after_end_repeat_off_stops_at_end() {
        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(backend_ok()));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg"), p("b.ogg")],
            start_index: 1,
        })
        .unwrap();
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("b.ogg")));

        let changed = e.advance_after_end();
        assert!(changed);
        assert_eq!(e.status, PlaybackStatus::Stopped);
        assert!(e.current_path.is_none());
    }

    #[test]
    fn on_tick_advances_when_sink_empty_and_playing() {
        let backend = backend_ok();
        backend
            .sink_empty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let append_calls = backend.append_calls.clone();

        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(backend));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg"), p("b.ogg")],
            start_index: 0,
        })
        .unwrap();
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));

        let before_appends = append_calls.load(std::sync::atomic::Ordering::Relaxed);
        let changed = e.on_tick();
        assert!(changed);
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("b.ogg")));
        let after_appends = append_calls.load(std::sync::atomic::Ordering::Relaxed);
        assert!(after_appends >= before_appends + 1);
    }

    #[test]
    fn play_at_pos_failure_does_not_corrupt_state() {
        let mut ok = backend_ok();
        ok.fail_append = false;

        let mut e = Engine::new_with_backend(false, RepeatMode::Off, Box::new(ok));
        e.on_command(PlayerCommand::LoadQueue {
            tracks: vec![p("a.ogg"), p("b.ogg")],
            start_index: 0,
        })
        .unwrap();
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));
        assert_eq!(e.queue.pos_in_order(), Some(0));

        let mut bad = backend_ok();
        bad.fail_append = true;
        e.backend = Box::new(bad);
        e.backend_available = true;

        let err = e.play_at_pos(1).unwrap_err();
        assert!(err.contains("decode failed"));
        assert_eq!(e.current_path.as_deref(), Some(std::path::Path::new("a.ogg")));
        assert_eq!(e.queue.pos_in_order(), Some(0));
        assert_eq!(e.status, PlaybackStatus::Playing);
    }
}
