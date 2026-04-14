use crate::error::AppResult;
use crate::paths::AppPaths;
use tracing_subscriber::EnvFilter;

pub struct LoggingGuards {
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
}

pub fn init(paths: &AppPaths) -> AppResult<LoggingGuards> {
    let file_appender = tracing_appender::rolling::never(&paths.logs_dir, "ost_player.log");
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let init_res = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .try_init();

    if let Err(e) = init_res {
        // Most common scenario in tests / multi-init paths: subscriber is already set.
        // Don't fail in that case; do fail for other unexpected init errors.
        let msg = e.to_string();
        if !msg.contains("already been set") {
            return Err(anyhow::anyhow!(msg).into());
        }
    }

    Ok(LoggingGuards {
        _file_guard: file_guard,
    })
}

