use crate::error::{AppError, AppResult};
use std::fs;
use std::path::{Path, PathBuf};

pub fn temp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    tmp.set_file_name(format!("{file_name}.tmp"));
    tmp
}

pub fn backup_path_for(path: &Path) -> PathBuf {
    let mut bak = path.to_path_buf();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    bak.set_file_name(format!("{file_name}.bak"));
    bak
}

/// If `path` is missing, attempt best-effort recovery from `.bak` or `.tmp`.
///
/// Recovery strategy (non-destructive):
/// - If `{path}.bak` exists, prefer restoring it to `path` (leaves `.tmp` untouched).
/// - Else if `{path}.tmp` exists, rename it to `path` (safe when `path` is missing).
/// - Otherwise do nothing.
pub fn recover_missing_final(path: &Path) -> AppResult<()> {
    if path.exists() {
        return Ok(());
    }

    let bak_path = backup_path_for(path);
    if bak_path.exists() {
        fs::rename(&bak_path, path).map_err(|e| AppError::Io {
            path: bak_path.clone(),
            source: e,
        })?;
        return Ok(());
    }

    let tmp_path = temp_path_for(path);
    if tmp_path.exists() {
        fs::rename(&tmp_path, path).map_err(|e| AppError::Io {
            path: tmp_path.clone(),
            source: e,
        })?;
    }

    Ok(())
}

pub fn write_text_safely(path: &Path, contents: &str) -> AppResult<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(|e| AppError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;

    let tmp_path = temp_path_for(path);
    fs::write(&tmp_path, contents).map_err(|e| AppError::Io {
        path: tmp_path.clone(),
        source: e,
    })?;

    // Best-effort durable + non-destructive replacement strategy:
    // - rename existing → .bak
    // - rename tmp → final
    // - remove .bak
    let bak_path = backup_path_for(path);
    if path.exists() {
        if bak_path.exists() {
            fs::remove_file(&bak_path).ok();
        }
        fs::rename(path, &bak_path).map_err(|e| AppError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        // Roll back if we already moved the original to `.bak`.
        if bak_path.exists() && !path.exists() {
            let _ = fs::rename(&bak_path, path);
        }
        // Best-effort cleanup: we failed to persist new data.
        let _ = fs::remove_file(&tmp_path);

        return Err(AppError::Io {
            path: path.to_path_buf(),
            source: e,
        });
    }

    if bak_path.exists() {
        fs::remove_file(&bak_path).ok();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn p(dir: &Path, name: &str) -> PathBuf {
        dir.join(name)
    }

    #[test]
    fn write_text_safely_creates_dir_and_leaves_no_tmp_or_bak() {
        let td = tempfile::tempdir().unwrap();
        let final_path = p(td.path(), "data/config.yaml");

        write_text_safely(&final_path, "a: 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&final_path).unwrap(), "a: 1\n");

        let tmp = temp_path_for(&final_path);
        let bak = backup_path_for(&final_path);
        assert!(!tmp.exists(), "tmp file should be cleaned up");
        assert!(!bak.exists(), "bak file should be cleaned up");
    }

    #[test]
    fn write_text_safely_replaces_existing_file() {
        let td = tempfile::tempdir().unwrap();
        let final_path = p(td.path(), "data/playlists.yaml");

        write_text_safely(&final_path, "first\n").unwrap();
        write_text_safely(&final_path, "second\n").unwrap();
        assert_eq!(std::fs::read_to_string(&final_path).unwrap(), "second\n");

        let tmp = temp_path_for(&final_path);
        let bak = backup_path_for(&final_path);
        assert!(!tmp.exists());
        assert!(!bak.exists());
    }

    #[test]
    fn recover_missing_final_prefers_bak_over_tmp() {
        let td = tempfile::tempdir().unwrap();
        let final_path = p(td.path(), "data/state.yaml");
        std::fs::create_dir_all(final_path.parent().unwrap()).unwrap();

        let bak = backup_path_for(&final_path);
        let tmp = temp_path_for(&final_path);
        std::fs::write(&bak, "from_bak\n").unwrap();
        std::fs::write(&tmp, "from_tmp\n").unwrap();

        recover_missing_final(&final_path).unwrap();
        assert_eq!(std::fs::read_to_string(&final_path).unwrap(), "from_bak\n");
        assert!(!bak.exists());
        assert!(
            tmp.exists(),
            "tmp should remain untouched when bak was restored"
        );
    }
}
