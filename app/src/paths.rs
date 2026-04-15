use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub base_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub playlists_dir: PathBuf,
    pub config_path: PathBuf,
    pub playlists_path: PathBuf,
    pub state_path: PathBuf,
}

impl AppPaths {
    /// Resolve portable paths relative to the running executable.
    pub fn resolve() -> AppResult<Self> {
        let exe = std::env::current_exe().map_err(|e| AppError::Io {
            path: PathBuf::from("<current_exe>"),
            source: e,
        })?;
        let base_dir = exe
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let data_dir = base_dir.join("data");
        let cache_dir = data_dir.join("cache");
        let logs_dir = data_dir.join("logs");
        let playlists_dir = data_dir.join("playlists");
        let config_path = data_dir.join("config.yaml");
        let playlists_path = data_dir.join("playlists.yaml");
        let state_path = data_dir.join("state.yaml");

        Ok(Self {
            base_dir,
            data_dir,
            cache_dir,
            logs_dir,
            playlists_dir,
            config_path,
            playlists_path,
            state_path,
        })
    }

    pub fn ensure_data_dirs(&self) -> AppResult<()> {
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| AppError::Io {
            path: self.cache_dir.clone(),
            source: e,
        })?;
        std::fs::create_dir_all(&self.logs_dir).map_err(|e| AppError::Io {
            path: self.logs_dir.clone(),
            source: e,
        })?;
        std::fs::create_dir_all(&self.playlists_dir).map_err(|e| AppError::Io {
            path: self.playlists_dir.clone(),
            source: e,
        })?;
        Ok(())
    }

    /// Guard: portable mode requires the app folder to be writable.
    ///
    /// If the directory is not writable (e.g., under Program Files), we fail fast with an
    /// actionable error so we don't run in a surprising "read-only config" mode.
    pub fn ensure_writable(&self) -> AppResult<()> {
        self.ensure_data_dirs()?;

        let probe_path = self.data_dir.join(".write_test.tmp");
        std::fs::write(&probe_path, b"probe").map_err(|e| AppError::PortableNotWritable {
            data_dir: self.data_dir.clone(),
            source: e,
        })?;
        std::fs::remove_file(&probe_path).map_err(|e| AppError::PortableNotWritable {
            data_dir: self.data_dir.clone(),
            source: e,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_for(base_dir: &Path) -> AppPaths {
        let base_dir = base_dir.to_path_buf();
        let data_dir = base_dir.join("data");
        AppPaths {
            base_dir,
            data_dir: data_dir.clone(),
            cache_dir: data_dir.join("cache"),
            logs_dir: data_dir.join("logs"),
            playlists_dir: data_dir.join("playlists"),
            config_path: data_dir.join("config.yaml"),
            playlists_path: data_dir.join("playlists.yaml"),
            state_path: data_dir.join("state.yaml"),
        }
    }

    #[test]
    fn ensure_data_dirs_creates_expected_directories() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        paths.ensure_data_dirs().unwrap();

        assert!(paths.cache_dir.exists());
        assert!(paths.logs_dir.exists());
        assert!(paths.playlists_dir.exists());
    }

    #[test]
    fn ensure_writable_creates_dirs_and_removes_probe() {
        let td = tempfile::tempdir().unwrap();
        let paths = paths_for(td.path());
        paths.ensure_writable().unwrap();

        assert!(paths.data_dir.exists());
        assert!(paths.cache_dir.exists());
        assert!(paths.logs_dir.exists());
        assert!(paths.playlists_dir.exists());

        let probe = paths.data_dir.join(".write_test.tmp");
        assert!(
            !probe.exists(),
            "probe file should be removed after successful writability check"
        );
    }
}
