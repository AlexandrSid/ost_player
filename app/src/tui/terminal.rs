use crate::command_bus::{BusMessage, CommandBus, CommandSource};
use crate::error::{AppError, AppResult};
use crate::hotkeys::HotkeysService;
use crate::tui::action::Action;
use crate::tui::app::TuiApp;
use crossterm::{
    cursor,
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, Terminal};
use std::io;
use std::time::{Duration, Instant};

struct TerminalRestoreGuard {
    raw_mode_enabled: bool,
    alt_screen_enabled: bool,
    focus_change_enabled: bool,
}

impl TerminalRestoreGuard {
    fn enter() -> AppResult<Self> {
        enable_raw_mode().map_err(|e| AppError::Io {
            path: "<enable_raw_mode>".into(),
            source: e,
        })?;

        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            let _ = execute!(stdout, cursor::Show);
            return Err(AppError::Io {
                path: "<enter_alt_screen>".into(),
                source: e,
            });
        }
        // Keep cursor visible: some terminals hide it on alt-screen entry.
        let _ = execute!(stdout, cursor::Show);

        // Best-effort: enables FocusGained/FocusLost reporting where supported.
        // Ignore errors to keep cross-platform behavior.
        let focus_change_enabled = execute!(stdout, event::EnableFocusChange).is_ok();

        Ok(Self {
            raw_mode_enabled: true,
            alt_screen_enabled: true,
            focus_change_enabled,
        })
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.focus_change_enabled {
            let _ = execute!(stdout, event::DisableFocusChange);
        }
        if self.alt_screen_enabled {
            let _ = execute!(stdout, LeaveAlternateScreen);
        }
        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
        }
        let _ = execute!(stdout, cursor::Show);
    }
}

pub fn run(app: &mut TuiApp) -> AppResult<()> {
    let _guard = TerminalRestoreGuard::enter()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(anyhow::Error::new)?;

    let bus = CommandBus::new();
    let bus_tx = bus.sender();

    let _hotkeys = match HotkeysService::start(&app.state.cfg.hotkeys, bus_tx.clone()) {
        Ok(svc) => svc,
        Err(msg) => {
            // Hotkeys are optional; keep the app usable.
            app.apply(Action::SetStatus(format!("hotkeys disabled: {msg}")))?;
            None
        }
    };

    let res: AppResult<()> = (|| {
        let mut focused = true;
        let mut minimized = false;
        let mut needs_redraw = true;
        let mut last_redraw_at = Instant::now() - Duration::from_secs(60);

        // Tell the player our initial best-effort UI activity state.
        bus.emit_action(
            CommandSource::System,
            Action::PlayerSetUiActivity { focused, minimized },
        );

        loop {
            while let Some(msg) = bus.try_recv() {
                needs_redraw = true;
                if handle_bus_message(app, msg)? {
                    return Ok(());
                }
            }

            let now = Instant::now();
            if is_redraw_due(app, focused, minimized, last_redraw_at, now) {
                needs_redraw = true;
            }

            if needs_redraw {
                terminal
                    .draw(|f| crate::tui::ui::draw(f, app))
                    .map_err(anyhow::Error::new)?;
                last_redraw_at = Instant::now();
                needs_redraw = false;

                // Allow periodic screen ticks (e.g. status clearing).
                if let Some(action) = app.tick()? {
                    bus.emit_action(CommandSource::System, action);
                }
            }

            while let Some(msg) = bus.try_recv() {
                if handle_bus_message(app, msg)? {
                    break;
                }
            }

            let poll_timeout = next_poll_timeout(app, focused, minimized, last_redraw_at);
            if event::poll(poll_timeout).map_err(|e| AppError::Io {
                path: "<event_poll>".into(),
                source: e,
            })? {
                match event::read().map_err(|e| AppError::Io {
                    path: "<event_read>".into(),
                    source: e,
                })? {
                    Event::Key(key) => {
                        if key.kind != KeyEventKind::Press {
                            continue;
                        }
                        needs_redraw = true;
                        if let Some(action) = app.on_key(key)? {
                            bus.emit_action(CommandSource::Tui, action);
                        }
                    }
                    Event::Paste(text) => {
                        needs_redraw = true;
                        if let Some(action) = app.on_paste(&text)? {
                            bus.emit_action(CommandSource::Tui, action);
                        }
                    }
                    Event::FocusGained => {
                        focused = true;
                        minimized = false;
                        needs_redraw = true;
                        bus.emit_action(
                            CommandSource::System,
                            Action::PlayerSetUiActivity { focused, minimized },
                        );
                    }
                    Event::FocusLost => {
                        focused = false;
                        needs_redraw = true;
                        bus.emit_action(
                            CommandSource::System,
                            Action::PlayerSetUiActivity { focused, minimized },
                        );
                    }
                    Event::Resize(w, h) => {
                        // Best-effort minimize detection: some terminals report 0x0 when minimized.
                        let next_minimized = w == 0 || h == 0;
                        if next_minimized != minimized {
                            minimized = next_minimized;
                            needs_redraw = true;
                            bus.emit_action(
                                CommandSource::System,
                                Action::PlayerSetUiActivity { focused, minimized },
                            );
                        }
                    }
                    _ => {}
                }
            } else {
                // Timeout -> redraw only if cadence says it's due.
                let now = Instant::now();
                if is_redraw_due(app, focused, minimized, last_redraw_at, now) {
                    needs_redraw = true;
                }
            }
        }
    })();

    // Best-effort deterministic shutdown. Never panic; allow process to exit even on timeout.
    if let Err(msg) = app.shutdown_player(Duration::from_secs(2)) {
        app.apply(Action::SetStatus(msg)).ok();
    }

    res
}

fn refresh_interval(app: &TuiApp, focused: bool, minimized: bool) -> Option<Duration> {
    if minimized {
        return None;
    }
    let playing = app.state.player.status == crate::player::PlaybackStatus::Playing;
    if playing && focused {
        Some(Duration::from_secs(1))
    } else if playing ^ focused {
        Some(Duration::from_secs(5))
    } else {
        None
    }
}

fn is_redraw_due(
    app: &TuiApp,
    focused: bool,
    minimized: bool,
    last_redraw_at: Instant,
    now: Instant,
) -> bool {
    let Some(interval) = refresh_interval(app, focused, minimized) else {
        return false;
    };
    now.duration_since(last_redraw_at) >= interval
}

fn next_poll_timeout(
    app: &TuiApp,
    focused: bool,
    minimized: bool,
    last_redraw_at: Instant,
) -> Duration {
    // Without a cross-platform "select" over terminal events and our command bus,
    // keep polling bounded so hotkeys remain responsive.
    let bus_check_interval = if minimized {
        Duration::from_millis(1000)
    } else {
        Duration::from_millis(200)
    };

    let Some(refresh) = refresh_interval(app, focused, minimized) else {
        return bus_check_interval;
    };
    let next_refresh_at = last_redraw_at + refresh;
    let now = Instant::now();
    let remaining = next_refresh_at.saturating_duration_since(now);
    remaining.min(bus_check_interval)
}

fn handle_action(app: &mut TuiApp, action: Action) -> AppResult<bool> {
    let quit = matches!(action, Action::Quit);
    app.apply(action)?;
    Ok(quit)
}

fn handle_bus_message(app: &mut TuiApp, msg: BusMessage) -> AppResult<bool> {
    match msg {
        BusMessage::Command(cmd) => handle_action(app, cmd.action),
    }
}
