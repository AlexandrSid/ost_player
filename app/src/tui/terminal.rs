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
use std::time::Duration;

struct TerminalRestoreGuard {
    raw_mode_enabled: bool,
    alt_screen_enabled: bool,
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

        Ok(Self {
            raw_mode_enabled: true,
            alt_screen_enabled: true,
        })
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
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
        loop {
            while let Some(msg) = bus.try_recv() {
                if handle_bus_message(app, msg)? {
                    return Ok(());
                }
            }

            terminal
                .draw(|f| crate::tui::ui::draw(f, app))
                .map_err(anyhow::Error::new)?;

            // Allow periodic screen ticks (e.g. transient status clearing later).
            if let Some(action) = app.tick()? {
                bus.emit_action(CommandSource::System, action);
            }

            while let Some(msg) = bus.try_recv() {
                if handle_bus_message(app, msg)? {
                    break;
                }
            }

            if event::poll(Duration::from_millis(50)).map_err(|e| AppError::Io {
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
                        if let Some(action) = app.on_key(key)? {
                            bus.emit_action(CommandSource::Tui, action);
                        }
                    }
                    Event::Paste(text) => {
                        if let Some(action) = app.on_paste(&text)? {
                            bus.emit_action(CommandSource::Tui, action);
                        }
                    }
                    _ => {}
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
