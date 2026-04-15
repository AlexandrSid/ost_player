use crate::indexer::model::{
    FolderScanEntry, IndexIssue, IndexIssueKind, IndexReport, LibraryIndex, ScanOptions,
    TrackEntry, TrackId,
};
use std::collections::{BTreeSet, HashSet};
use std::ffi::{OsStr, OsString};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub fn scan_library(roots: &[String], options: &ScanOptions) -> LibraryIndex {
    let folders = roots
        .iter()
        .map(|p| FolderScanEntry {
            path: p.clone(),
            root_only: false,
        })
        .collect::<Vec<_>>();
    scan_library_folders(&folders, options)
}

pub fn scan_library_folders(folders: &[FolderScanEntry], options: &ScanOptions) -> LibraryIndex {
    let options = options.normalized();
    let mut report = IndexReport {
        roots_total: folders.len(),
        ..Default::default()
    };

    let mut tracks: Vec<TrackEntry> = Vec::new();

    // Dedup by canonical path when possible.
    let mut seen_paths: BTreeSet<OsString> = BTreeSet::new();
    // Optional fallback: (root identity, relative path within root (case-normalized on Windows), size).
    let mut seen_rel_size: HashSet<(OsString, OsString, u64)> = HashSet::new();

    for folder in folders {
        let root_path = PathBuf::from(&folder.path);
        if !root_path.exists() {
            report.record_issue(IndexIssue {
                kind: IndexIssueKind::MissingFolder,
                path: root_path,
                message: "folder does not exist".to_string(),
            });
            continue;
        }

        let root_canon = match root_path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                report.record_issue(IndexIssue {
                    kind: classify_io_issue(&e, IndexIssueKind::CanonicalizeFailed),
                    path: root_path.clone(),
                    message: format!("failed to canonicalize folder: {e}"),
                });
                // Best-effort: continue scanning with non-canonical root.
                root_path
            }
        };

        let root_key = canonical_dedup_key(&root_canon);
        if let Err(e) = scan_dir(
            &root_canon,
            &root_canon,
            root_key.as_os_str(),
            &options,
            !folder.root_only,
            &mut tracks,
            &mut seen_paths,
            &mut seen_rel_size,
            &mut report,
        ) {
            report.record_issue(IndexIssue {
                kind: classify_io_issue(&e, IndexIssueKind::ReadDirFailed),
                path: root_canon.clone(),
                message: format!("failed to scan folder: {e}"),
            });
            continue;
        }

        report.roots_ok += 1;
    }

    // Deterministic sort: prefer rel_path when we have it, then absolute path.
    tracks.sort_by(|a, b| {
        let a_rel = a.rel_path.as_deref().map(|s| s.to_ascii_lowercase());
        let b_rel = b.rel_path.as_deref().map(|s| s.to_ascii_lowercase());

        match (a_rel, b_rel) {
            (Some(ak), Some(bk)) => match ak.cmp(&bk) {
                std::cmp::Ordering::Equal => {}
                ord => return ord,
            },
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            (None, None) => {}
        }

        match os_str_sort_key(a.path.as_os_str()).cmp(&os_str_sort_key(b.path.as_os_str())) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match a.size_bytes.cmp(&b.size_bytes) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        a.id.cmp(&b.id)
    });

    report.tracks_total = tracks.len();
    LibraryIndex {
        schema_version: 1,
        tracks,
        report,
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_dir(
    root: &Path,
    dir: &Path,
    root_key: &OsStr,
    options: &ScanOptions,
    recurse: bool,
    out_tracks: &mut Vec<TrackEntry>,
    seen_paths: &mut BTreeSet<OsString>,
    seen_rel_size: &mut HashSet<(OsString, OsString, u64)>,
    report: &mut IndexReport,
) -> Result<(), std::io::Error> {
    let entries = std::fs::read_dir(dir)?;

    for entry_res in entries {
        let entry = match entry_res {
            Ok(v) => v,
            Err(e) => {
                report.record_issue(IndexIssue {
                    kind: classify_io_issue(&e, IndexIssueKind::ReadDirFailed),
                    path: dir.to_path_buf(),
                    message: format!("failed to read directory entry: {e}"),
                });
                continue;
            }
        };

        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(v) => v,
            Err(e) => {
                report.record_issue(IndexIssue {
                    kind: classify_io_issue(&e, IndexIssueKind::MetadataFailed),
                    path: path.clone(),
                    message: format!("failed to read file type: {e}"),
                });
                continue;
            }
        };

        if ft.is_dir() {
            if recurse {
                if let Err(e) = scan_dir(
                    root,
                    &path,
                    root_key,
                    options,
                    recurse,
                    out_tracks,
                    seen_paths,
                    seen_rel_size,
                    report,
                ) {
                    report.record_issue(IndexIssue {
                        kind: classify_io_issue(&e, IndexIssueKind::ReadDirFailed),
                        path: path.clone(),
                        message: format!("failed to scan directory: {e}"),
                    });
                }
            }
            continue;
        }

        if !ft.is_file() {
            continue;
        }

        report.files_seen += 1;

        if !is_supported_audio(&path, &options.supported_extensions) {
            report.skipped_ext += 1;
            continue;
        }

        let meta = match std::fs::metadata(&path) {
            Ok(v) => v,
            Err(e) => {
                report.record_issue(IndexIssue {
                    kind: classify_io_issue(&e, IndexIssueKind::MetadataFailed),
                    path: path.clone(),
                    message: format!("failed to read metadata: {e}"),
                });
                continue;
            }
        };

        let size = meta.len();
        if size < options.min_size_bytes {
            report.skipped_small += 1;
            continue;
        }

        let force_canon_fail = options.force_canonicalize_fail;
        let (best_path, canon_key) = if force_canon_fail {
            (path.clone(), None)
        } else {
            match path.canonicalize() {
                Ok(p) => {
                    let key = canonical_dedup_key(&p);
                    (p, Some(key))
                }
                Err(e) => {
                    report.record_issue(IndexIssue {
                        kind: classify_io_issue(&e, IndexIssueKind::CanonicalizeFailed),
                        path: path.clone(),
                        message: format!(
                            "failed to canonicalize file (will use best-effort path): {e}"
                        ),
                    });
                    (path.clone(), None)
                }
            }
        };

        if let Some(key) = canon_key {
            let inserted = seen_paths.insert(key);
            if !inserted {
                report.deduped += 1;
                continue;
            }
        } else if options.allow_name_size_fallback_dedup {
            if let Some(rel_key) = fallback_rel_key(root, &best_path) {
                if !seen_rel_size.insert((root_key.to_os_string(), rel_key, size)) {
                    report.deduped += 1;
                    continue;
                }
            }
        }

        let rel_path =
            best_effort_rel_path(root, &best_path).map(|p| p.to_string_lossy().to_string());

        out_tracks.push(TrackEntry {
            id: track_id_for_path(&best_path),
            path: best_path,
            rel_path,
            size_bytes: size,
        });
    }

    Ok(())
}

fn canonical_dedup_key(path: &Path) -> OsString {
    // On Windows treat paths as case-insensitive when they are valid Unicode.
    #[cfg(windows)]
    {
        if let Some(s) = path.as_os_str().to_str() {
            fn is_drive_letter_path(s: &str) -> bool {
                let b = s.as_bytes();
                if b.len() < 3 {
                    return false;
                }
                let is_alpha = (b[0] >= b'A' && b[0] <= b'Z') || (b[0] >= b'a' && b[0] <= b'z');
                is_alpha && b[1] == b':' && b[2] == b'\\'
            }

            // Normalize common Windows path variations so dedup is stable:
            // - strip verbatim prefix (\\?\) that `canonicalize()` may add
            // - convert verbatim UNC prefix (\\?\UNC\) back to normal UNC (\\)
            // - normalize slashes
            let mut norm = s.replace('/', "\\");
            if let Some(rest) = norm.strip_prefix(r"\\?\UNC\") {
                // `\\?\UNC\server\share\path` => `\\server\share\path`
                norm = format!("\\\\{rest}");
            } else if let Some(rest) = norm.strip_prefix(r"\\?\") {
                // Only strip `\\?\` for drive-letter paths; keep it for device paths
                // like `\\?\GLOBALROOT\...` or `\\?\Volume{GUID}\...`.
                if is_drive_letter_path(rest) {
                    // `\\?\C:\path` => `C:\path`
                    norm = rest.to_string();
                }
            }
            return OsString::from(norm.to_ascii_lowercase());
        }
        path.as_os_str().to_os_string()
    }
    #[cfg(not(windows))]
    {
        path.as_os_str().to_os_string()
    }
}

fn fallback_rel_key(root: &Path, path: &Path) -> Option<OsString> {
    let rel = path.strip_prefix(root).ok()?;
    #[cfg(windows)]
    {
        if let Some(s) = rel.as_os_str().to_str() {
            return Some(OsString::from(s.to_ascii_lowercase()));
        }
        Some(rel.as_os_str().to_os_string())
    }
    #[cfg(not(windows))]
    {
        Some(rel.as_os_str().to_os_string())
    }
}

fn os_str_sort_key(s: &OsStr) -> Vec<u8> {
    // Non-lossy stable ordering across platforms.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut out = Vec::new();
        for w in s.encode_wide() {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes().to_vec()
    }
}

fn is_supported_audio(path: &Path, supported_extensions: &[String]) -> bool {
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext.is_empty() {
        return false;
    }
    supported_extensions.iter().any(|e| e == &ext)
}

fn best_effort_rel_path<'a>(root: &'a Path, path: &'a Path) -> Option<&'a Path> {
    if let Ok(rel) = path.strip_prefix(root) {
        return Some(rel);
    }
    None
}

fn track_id_for_path(path: &Path) -> TrackId {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Non-lossy hash; on Windows, case-normalize when Unicode.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        if let Some(s) = path.as_os_str().to_str() {
            s.to_ascii_lowercase().hash(&mut h);
        } else {
            for w in path.as_os_str().encode_wide() {
                w.hash(&mut h);
            }
        }
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().hash(&mut h);
    }
    TrackId(h.finish())
}

fn classify_io_issue(err: &std::io::Error, default: IndexIssueKind) -> IndexIssueKind {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => IndexIssueKind::MissingFolder,
        ErrorKind::PermissionDenied => IndexIssueKind::PermissionDenied,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_dummy_file(path: &std::path::Path, size_bytes: usize) {
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let data = vec![0u8; size_bytes.max(1)];
        std::fs::write(path, data).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn canonical_dedup_key_normalizes_verbatim_unc_prefix() {
        let p = std::path::PathBuf::from(r"\\?\UNC\Server\Share\Music\Track.ogg");
        let key = canonical_dedup_key(&p);
        assert_eq!(key.to_str().unwrap(), r"\\server\share\music\track.ogg");
    }

    #[test]
    #[cfg(windows)]
    fn canonical_dedup_key_strips_verbatim_prefix_for_local_paths() {
        let p = std::path::PathBuf::from(r"\\?\C:\Music\Track.ogg");
        let key = canonical_dedup_key(&p);
        assert_eq!(key.to_str().unwrap(), r"c:\music\track.ogg");
    }

    #[test]
    #[cfg(windows)]
    fn canonical_dedup_key_preserves_verbatim_prefix_for_device_paths() {
        let p = std::path::PathBuf::from(
            r"\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy1\Windows\System32",
        );
        let key = canonical_dedup_key(&p);
        assert_eq!(
            key.to_str().unwrap(),
            r"\\?\globalroot\device\harddiskvolumeshadowcopy1\windows\system32"
        );
    }

    #[test]
    fn root_only_true_scans_only_top_level_files() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().join("music");
        let root_track = root.join("a.ogg");
        let sub_track = root.join("sub").join("b.ogg");
        write_dummy_file(&root_track, 16);
        write_dummy_file(&sub_track, 16);

        let opts = ScanOptions {
            supported_extensions: vec!["ogg".to_string()],
            min_size_bytes: 1,
            allow_name_size_fallback_dedup: true,
            force_canonicalize_fail: false,
        };

        let folders = vec![FolderScanEntry {
            path: root.to_string_lossy().to_string(),
            root_only: true,
        }];

        let index = scan_library_folders(&folders, &opts);
        assert_eq!(index.report.issues.len(), 0);
        assert_eq!(index.tracks.len(), 1);
        assert!(index.tracks[0].path.ends_with("a.ogg"));
    }

    #[test]
    fn root_only_false_scans_recursively() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().join("music");
        let root_track = root.join("a.ogg");
        let sub_track = root.join("sub").join("b.ogg");
        write_dummy_file(&root_track, 16);
        write_dummy_file(&sub_track, 16);

        let opts = ScanOptions {
            supported_extensions: vec!["ogg".to_string()],
            min_size_bytes: 1,
            allow_name_size_fallback_dedup: true,
            force_canonicalize_fail: false,
        };

        let folders = vec![FolderScanEntry {
            path: root.to_string_lossy().to_string(),
            root_only: false,
        }];

        let index = scan_library_folders(&folders, &opts);
        assert_eq!(index.report.issues.len(), 0);
        assert_eq!(index.tracks.len(), 2);
    }
}
