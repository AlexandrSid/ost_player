use ost_player::{config, logging, paths::AppPaths, playlists, tui};

fn main() -> anyhow::Result<()> {
    let paths = AppPaths::resolve()?;
    paths.ensure_writable()?;

    let cfg = config::io::load_or_create(&paths)?;
    let _log_guards = logging::init(&paths, &cfg)?;

    #[cfg(windows)]
    ost_player::windows_icon::best_effort_set_console_window_icon_from_resource_id(1);

    let pls = playlists::io::load_or_create(&paths)?;
    tracing::debug!(
        min_size_bytes = cfg.settings.min_size_bytes,
        shuffle = cfg.settings.shuffle,
        repeat = ?cfg.settings.repeat,
        folders = cfg.folders.len(),
        playlists = pls.playlists.len(),
        "config + playlists loaded"
    );

    tui::run(paths, cfg, pls)?;
    Ok(())
}
