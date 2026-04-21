use crate::config::{AppConfig, LoggingLevel};
use crate::error::AppResult;
use crate::paths::AppPaths;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing_subscriber::EnvFilter;

pub struct LoggingGuards {
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
}

pub fn init(paths: &AppPaths, cfg: &AppConfig) -> AppResult<LoggingGuards> {
    cleanup_old_logs(&paths.logs_dir, cfg.logging.retention_days);

    let file_appender = BucketedFileAppender::new(paths.logs_dir.clone());
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = build_env_filter(cfg);

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

fn build_env_filter(cfg: &AppConfig) -> EnvFilter {
    // If `RUST_LOG` is set, give the operator full control.
    if std::env::var("RUST_LOG")
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
    {
        return EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    }

    let app_level = match cfg.logging.default_level {
        LoggingLevel::Default => "info",
        LoggingLevel::Debug => "debug",
        LoggingLevel::Trace => "trace",
    };

    // Policy:
    // - Always keep global ERROR on (all crates).
    // - Allow ost_player domain events at chosen level.
    // - Suppress noisy dependencies by default.
    // Note: users can override all of this via RUST_LOG.
    let filter = format!(
        "error,ost_player={app_level},rodio=error,symphonia=error,symphonia_bundle_mp3=error,symphonia_bundle_ogg=error"
    );
    EnvFilter::new(filter)
}

fn cleanup_old_logs(logs_dir: &Path, retention_days: u64) {
    let Ok(entries) = fs::read_dir(logs_dir) else {
        return;
    };

    let retention = Duration::from_secs(retention_days.saturating_mul(24 * 60 * 60));
    let Ok(now) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) else {
        return;
    };
    let now = SystemTime::UNIX_EPOCH + now;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_bucket_log_file_name(name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(mtime) else {
            continue;
        };
        if age > retention {
            let _ = fs::remove_file(&path);
        }
    }
}

fn is_bucket_log_file_name(name: &str) -> bool {
    // Expected: YYYY-MM-01_10.log / YYYY-MM-11_20.log / YYYY-MM-21_eom.log
    let Some(stem) = name.strip_suffix(".log") else {
        return false;
    };
    let Some((date_part, suffix)) = stem.split_once('_') else {
        return false;
    };
    let expected_suffix = match suffix {
        "10" | "20" | "eom" => suffix,
        _ => return false,
    };

    // date_part must be YYYY-MM-DD
    if date_part.len() != 10 {
        return false;
    }
    let bytes = date_part.as_bytes();
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return false;
    }
    let year_s = &date_part[0..4];
    let month_s = &date_part[5..7];
    let day_s = &date_part[8..10];
    if !year_s.chars().all(|c| c.is_ascii_digit())
        || !month_s.chars().all(|c| c.is_ascii_digit())
        || !day_s.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let Ok(month) = month_s.parse::<u8>() else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }

    // Enforce our bucket tokens and their required suffix.
    matches!(
        (day_s, expected_suffix),
        ("01", "10") | ("11", "20") | ("21", "eom")
    )
}

struct BucketedFileAppender {
    logs_dir: PathBuf,
    current_bucket: Option<String>,
    file: Option<fs::File>,
}

impl BucketedFileAppender {
    fn new(logs_dir: PathBuf) -> Self {
        Self {
            logs_dir,
            current_bucket: None,
            file: None,
        }
    }

    fn ensure_open_for_now(&mut self) -> io::Result<()> {
        let bucket = current_bucket_name();
        if self.current_bucket.as_deref() == Some(bucket.as_str()) && self.file.is_some() {
            return Ok(());
        }

        let path = self.logs_dir.join(format!("{bucket}.log"));
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        self.file = Some(file);
        self.current_bucket = Some(bucket);
        Ok(())
    }
}

impl Write for BucketedFileAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.ensure_open_for_now()?;
        match self.file.as_mut() {
            Some(f) => f.write(buf),
            None => Ok(0),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(f) = self.file.as_mut() {
            f.flush()?;
        }
        Ok(())
    }
}

fn current_bucket_name() -> String {
    use time::OffsetDateTime;

    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let y = now.year();
    let m = now.month() as u8;
    let d = now.day();

    // 3 buckets/month: 01-10, 11-20, 21-eom
    if d <= 10 {
        format!("{y:04}-{m:02}-01_10")
    } else if d <= 20 {
        format!("{y:04}-{m:02}-11_20")
    } else {
        format!("{y:04}-{m:02}-21_eom")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, LoggingLevel};
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_rust_log<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var("RUST_LOG").ok();

        match value {
            Some(v) => std::env::set_var("RUST_LOG", v),
            None => std::env::remove_var("RUST_LOG"),
        }

        struct Restore(Option<String>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.as_deref() {
                    Some(v) => std::env::set_var("RUST_LOG", v),
                    None => std::env::remove_var("RUST_LOG"),
                }
            }
        }

        let _restore = Restore(prev);
        f()
    }

    #[test]
    fn build_env_filter_uses_rust_log_when_set_and_non_empty() {
        let v = "ost_player=warn";
        with_rust_log(Some(v), || {
            let cfg = AppConfig {
                logging: crate::config::LoggingConfig {
                    default_level: LoggingLevel::Trace,
                    ..Default::default()
                },
                ..Default::default()
            };
            let built = build_env_filter(&cfg);
            assert_eq!(
                built.to_string(),
                EnvFilter::new(v).to_string(),
                "expected build_env_filter to delegate to RUST_LOG when set"
            );
        })
    }

    #[test]
    fn build_env_filter_ignores_blank_rust_log_and_uses_config_policy() {
        with_rust_log(Some("   "), || {
            let cfg = AppConfig {
                logging: crate::config::LoggingConfig {
                    default_level: LoggingLevel::Debug,
                    ..Default::default()
                },
                ..Default::default()
            };
            let f = build_env_filter(&cfg);
            let s = f.to_string();
            assert!(s.contains("error"), "got: {s}");
            assert!(s.contains("ost_player=debug"), "got: {s}");
            assert!(s.contains("rodio=error"), "got: {s}");
            assert!(s.contains("symphonia=error"), "got: {s}");
        })
    }

    #[test]
    fn current_bucket_name_has_expected_format_and_prefix_for_current_month() {
        use time::OffsetDateTime;

        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let y = now.year();
        let m = now.month() as u8;

        let b = current_bucket_name();
        assert!(
            b.starts_with(&format!("{y:04}-{m:02}-")),
            "bucket should start with current year-month; got: {b}"
        );
        assert!(
            b.ends_with("01_10") || b.ends_with("11_20") || b.ends_with("21_eom"),
            "bucket should end with a known suffix; got: {b}"
        );
    }

    #[test]
    fn cleanup_old_logs_deletes_log_files_when_retention_is_zero_and_age_is_positive() {
        let td = tempfile::tempdir().unwrap();

        let log_path = td.path().join("2026-04-01_10.log");
        let keep_other_log = td.path().join("other.log");
        let keep_txt = td.path().join("b.txt");
        fs::write(&log_path, b"x").unwrap();
        fs::write(&keep_other_log, b"y").unwrap();
        fs::write(&keep_txt, b"y").unwrap();

        // Ensure log file's mtime is in the past (even slightly) relative to "now" inside cleanup.
        thread::sleep(Duration::from_millis(10));

        cleanup_old_logs(td.path(), 0);

        assert!(
            !log_path.exists(),
            "bucket-pattern log file should be deleted"
        );
        assert!(
            keep_other_log.exists(),
            "non-bucket .log files should be ignored"
        );
        assert!(keep_txt.exists(), "non-.log files should be ignored");
    }

    #[test]
    fn cleanup_old_logs_keeps_recent_logs_when_retention_is_large() {
        let td = tempfile::tempdir().unwrap();

        let log_path = td.path().join("2026-04-11_20.log");
        fs::write(&log_path, b"x").unwrap();

        cleanup_old_logs(td.path(), 10_000);

        assert!(log_path.exists(), "recent.log should be kept");
    }

    #[test]
    fn is_bucket_log_file_name_validates_bucket_patterns_strictly() {
        assert!(is_bucket_log_file_name("2026-04-01_10.log"));
        assert!(is_bucket_log_file_name("2026-04-11_20.log"));
        assert!(is_bucket_log_file_name("2026-04-21_eom.log"));

        assert!(!is_bucket_log_file_name("2026-04-01_10.txt"));
        assert!(!is_bucket_log_file_name("2026-4-01_10.log"));
        assert!(!is_bucket_log_file_name("2026-13-01_10.log"));
        assert!(!is_bucket_log_file_name("2026-04-00_10.log"));
        assert!(!is_bucket_log_file_name("2026-04-01_20.log"));
        assert!(!is_bucket_log_file_name("2026-04-11_10.log"));
        assert!(!is_bucket_log_file_name("2026-04-21_20.log"));
        assert!(!is_bucket_log_file_name("other.log"));
    }
}
