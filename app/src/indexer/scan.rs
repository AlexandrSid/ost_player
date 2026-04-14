use crate::indexer::model::{
    IndexIssue, IndexIssueKind, IndexReport, LibraryIndex, ScanOptions, TrackEntry, TrackId,
};
use std::collections::{BTreeSet, HashSet};
use std::ffi::{OsStr, OsString};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use serde_json;

// #region agent log
#[allow(dead_code)]
fn agent_debug_log(hypothesis_id: &str, location: &str, message: &str, data: serde_yaml::Value) {
    // Enable only when explicitly requested to keep normal runs clean.
    if std::env::var_os("OST_PLAYER_DEBUG_DEDUP").is_none() {
        return;
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let payload = serde_yaml::Value::Mapping({
        let mut m = serde_yaml::Mapping::new();
        m.insert("sessionId".into(), "9686b3".into());
        m.insert("runId".into(), "dedup-test".into());
        m.insert("hypothesisId".into(), hypothesis_id.into());
        m.insert("location".into(), location.into());
        m.insert("message".into(), message.into());
        m.insert("timestamp".into(), ts.into());
        m.insert("data".into(), data);
        m
    });

    // Write to workspace root debug log (one level above app/).
    let log_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("debug-9686b3.log");

    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        if let Ok(line) = serde_json::to_string(&payload) {
            let _ = std::io::Write::write_all(&mut f, line.as_bytes());
            let _ = std::io::Write::write_all(&mut f, b"\n");
        }
    }
}
// #endregion agent log

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
        // #region agent log
        agent_debug_log(
            "H_dedup_root",
            "indexer/scan.rs:scan_library",
            "root_input",
            serde_yaml::Value::Mapping({
                let mut m = serde_yaml::Mapping::new();
                m.insert("root".into(), root.to_string().into());
                m
            }),
        );
        // #endregion agent log
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
        // #region agent log
        agent_debug_log(
            "H_dedup_root",
            "indexer/scan.rs:scan_library",
            "root_canon_and_key",
            serde_yaml::Value::Mapping({
                let mut m = serde_yaml::Mapping::new();
                m.insert(
                    "root_canon".into(),
                    root_canon.to_string_lossy().to_string().into(),
                );
                m.insert("root_key".into(), root_key.to_string_lossy().to_string().into());
                m
            }),
        );
        // #endregion agent log
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

    // #region agent log
    agent_debug_log(
        "H_dedup_summary",
        "indexer/scan.rs:scan_library",
        "scan_complete",
        serde_yaml::Value::Mapping({
            let mut m = serde_yaml::Mapping::new();
            m.insert("tracks_len".into(), (tracks.len() as i64).into());
            m.insert("deduped".into(), (report.deduped as i64).into());
            m.insert("roots_total".into(), (report.roots_total as i64).into());
            m.insert("roots_ok".into(), (report.roots_ok as i64).into());
            m
        }),
    );
    // #endregion agent log

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
            // #region agent log
            let key_hash = {
                let mut hh = std::collections::hash_map::DefaultHasher::new();
                os_str_sort_key(key.as_os_str()).hash(&mut hh);
                hh.finish()
            };
            agent_debug_log(
                "H_dedup_file",
                "indexer/scan.rs:scan_dir_recursive",
                "file_canon_key",
                serde_yaml::Value::Mapping({
                    let mut m = serde_yaml::Mapping::new();
                    m.insert("path".into(), path.to_string_lossy().to_string().into());
                    m.insert(
                        "best_path".into(),
                        best_path.to_string_lossy().to_string().into(),
                    );
                    m.insert("canon_key".into(), key.to_string_lossy().to_string().into());
                    m.insert("canon_key_hash".into(), (key_hash as i64).into());
                    m
                }),
            );
            // #endregion agent log
            let inserted = seen_paths.insert(key);
            // #region agent log
            agent_debug_log(
                "H_dedup_file",
                "indexer/scan.rs:scan_dir_recursive",
                "seen_paths_insert_result",
                serde_yaml::Value::Mapping({
                    let mut m = serde_yaml::Mapping::new();
                    m.insert("inserted".into(), inserted.into());
                    m.insert("seen_paths_len".into(), (seen_paths.len() as i64).into());
                    m
                }),
            );
            // #endregion agent log
            if !inserted {
                // #region agent log
                agent_debug_log(
                    "H_dedup_file",
                    "indexer/scan.rs:scan_dir_recursive",
                    "dedup_skip",
                    serde_yaml::Value::Mapping({
                        let mut m = serde_yaml::Mapping::new();
                        m.insert(
                            "best_path".into(),
                            best_path.to_string_lossy().to_string().into(),
                        );
                        m
                    }),
                );
                // #endregion agent log
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
        // #region agent log
        agent_debug_log(
            "H_dedup_file",
            "indexer/scan.rs:scan_dir_recursive",
            "track_pushed",
            serde_yaml::Value::Mapping({
                let mut m = serde_yaml::Mapping::new();
                m.insert("out_tracks_len".into(), (out_tracks.len() as i64).into());
                m.insert(
                    "pushed_path".into(),
                    out_tracks
                        .last()
                        .map(|t| t.path.to_string_lossy().to_string())
                        .unwrap_or_default()
                        .into(),
                );
                m
            }),
        );
        // #endregion agent log
    }

    Ok(())
}

fn canonical_dedup_key(path: &Path) -> OsString {
    // On Windows treat paths as case-insensitive when they are valid Unicode.
    #[cfg(windows)]
    {
        if let Some(s) = path.as_os_str().to_str() {
            // Normalize common Windows path variations so dedup is stable:
            // - strip verbatim prefix (\\?\) that `canonicalize()` may add
            // - normalize slashes
            let mut norm = s.replace('/', "\\");
            if let Some(rest) = norm.strip_prefix(r"\\?\") {
                norm = rest.to_string();
            }
            return OsString::from(norm.to_ascii_lowercase());
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

