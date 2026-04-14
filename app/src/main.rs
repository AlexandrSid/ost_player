use ost_player::{config, logging, paths::AppPaths, playlists, tui};

fn main() -> anyhow::Result<()> {
    let paths = AppPaths::resolve()?;
    paths.ensure_writable()?;

    let _log_guards = logging::init(&paths)?;

    let cfg = config::io::load_or_create(&paths)?;
    let pls = playlists::io::load_or_create(&paths)?;
    tracing::info!(
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

