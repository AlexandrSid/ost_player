use crate::indexer::model::{
    IndexIssue, IndexIssueKind, IndexReport, LibraryIndex, ScanOptions, TrackEntry, TrackId,
};
use std::collections::{BTreeSet, HashSet};
use std::ffi::{OsStr, OsString};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub fn scan_library(roots: &[String], options: &ScanOptions) -> LibraryIndex {
    let options = options.normalized();
    let mut report = IndexReport::default();
    report.roots_total = roots.len();

    let mut tracks: Vec<TrackEntry> = Vec::new();

    // Dedup by canonical path when possible.
    let mut seen_paths: BTreeSet<OsString> = BTreeSet::new();
    // Optional fallback: (root identity, relative path within root (case-normalized on Windows), size).
    let mut seen_rel_size: HashSet<(OsString, OsString, u64)> = HashSet::new();

    for root in roots {
        let root_path = PathBuf::from(root);
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
        if let Err(e) = scan_dir_recursive(
            &root_canon,
            &root_canon,
            root_key.as_os_str(),
            &options,
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

fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    root_key: &OsStr,
    options: &ScanOptions,
    out_tracks: &mut Vec<TrackEntry>,
    seen_paths: &mut BTreeSet<OsString>,
    seen_rel_size: &mut HashSet<(OsString, OsString, u64)>,
    report: &mut IndexReport,
) -> Result<(), std::io::Error> {
    let entries = match std::fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };

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
            if let Err(e) = scan_dir_recursive(
                root,
                &path,
                root_key,
                options,
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

        let force_canon_fail =
            std::env::var_os("OST_PLAYER_TEST_FORCE_CANONICALIZE_FAIL").is_some();
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
            if !seen_paths.insert(key) {
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

        let rel_path = best_effort_rel_path(root, &best_path)
            .map(|p| p.to_string_lossy().to_string());

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
            return OsString::from(s.to_ascii_lowercase());
        }
        return path.as_os_str().to_os_string();
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
        return Some(rel.as_os_str().to_os_string());
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
        return out;
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        return s.as_bytes().to_vec();
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

