use crate::config::ScanDepth;
use crate::indexer::model::{
    FolderScanEntry, IndexIssue, IndexIssueKind, IndexReport, LibraryIndex, ScanOptions,
    TrackEntry, TrackId,
};
use std::collections::{BTreeSet, HashSet};
use std::ffi::{OsStr, OsString};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub fn compute_index_fingerprint(folders: &[FolderScanEntry], options: &ScanOptions) -> String {
    let opts = options.normalized();

    // Keep the fingerprint stable even if folder order in config changes.
    let mut entries = folders
        .iter()
        .map(|f| {
            (
                normalize_fingerprint_path(&f.path),
                scan_depth_tag(f.scan_depth),
                f.min_size_bytes,
            )
        })
        .collect::<Vec<_>>();
    entries.sort();

    // Use a deterministic hash (FNV-1a 64-bit). `DefaultHasher` is not stable across runs.
    let mut h = Fnv1a64::new();
    h.write_bytes(b"ost_player:index_fingerprint:v1\0");

    h.write_bytes(b"exts\0");
    for ext in opts.supported_extensions.iter() {
        h.write_str(ext);
        h.write_bytes(b"\0");
    }

    h.write_bytes(b"folders\0");
    for (path, depth_tag, min_size) in entries {
        h.write_str(&path);
        h.write_bytes(b"\0");
        h.write_u8(depth_tag);
        h.write_u64(min_size);
        h.write_bytes(b"\0");
    }

    // Include relevant option knobs even if currently constant in callers.
    h.write_u64(opts.min_size_bytes);
    h.write_bool(opts.allow_name_size_fallback_dedup);
    h.write_bool(opts.force_canonicalize_fail);

    format!("{:016x}", h.finish())
}

fn scan_depth_tag(d: ScanDepth) -> u8 {
    match d {
        ScanDepth::RootOnly => 0,
        ScanDepth::OneLevel => 1,
        ScanDepth::Recursive => 2,
    }
}

pub fn scan_library(roots: &[String], options: &ScanOptions) -> LibraryIndex {
    let folders = roots
        .iter()
        .map(|p| FolderScanEntry {
            path: p.clone(),
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: options.min_size_bytes,
        })
        .collect::<Vec<_>>();
    scan_library_folders(&folders, options)
}

pub fn scan_library_folders(folders: &[FolderScanEntry], options: &ScanOptions) -> LibraryIndex {
    let options = options.normalized();
    let fingerprint = compute_index_fingerprint(folders, &options);
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
        let max_depth = match folder.scan_depth {
            ScanDepth::RootOnly => Some(0usize),
            ScanDepth::OneLevel => Some(1usize),
            ScanDepth::Recursive => None,
        };
        let min_size_bytes = folder.min_size_bytes;
        if let Err(e) = scan_dir(
            &root_canon,
            &root_canon,
            root_key.as_os_str(),
            &options,
            min_size_bytes,
            0,
            max_depth,
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
        schema_version: LibraryIndex::SCHEMA_VERSION,
        index_fingerprint: fingerprint,
        tracks,
        report,
    }
}

fn normalize_fingerprint_path(p: &str) -> String {
    #[cfg(windows)]
    {
        p.trim().replace('/', "\\").to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        p.trim().to_string()
    }
}

#[derive(Debug, Clone)]
struct Fnv1a64(u64);

impl Fnv1a64 {
    fn new() -> Self {
        // FNV-1a 64-bit offset basis.
        Self(0xcbf29ce484222325)
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        // FNV-1a 64-bit prime.
        const PRIME: u64 = 0x00000100000001B3;
        for b in bytes {
            self.0 ^= *b as u64;
            self.0 = self.0.wrapping_mul(PRIME);
        }
    }

    fn write_str(&mut self, s: &str) {
        self.write_bytes(s.as_bytes())
    }

    fn write_u8(&mut self, v: u8) {
        self.write_bytes(&[v])
    }

    fn write_u64(&mut self, v: u64) {
        self.write_bytes(&v.to_le_bytes())
    }

    fn write_bool(&mut self, v: bool) {
        self.write_u8(if v { 1 } else { 0 })
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_dir(
    root: &Path,
    dir: &Path,
    root_key: &OsStr,
    options: &ScanOptions,
    min_size_bytes: u64,
    cur_depth: usize,
    max_depth: Option<usize>,
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
            let can_recurse = match max_depth {
                None => true,
                Some(max) => cur_depth < max,
            };
            if can_recurse {
                if let Err(e) = scan_dir(
                    root,
                    &path,
                    root_key,
                    options,
                    min_size_bytes,
                    cur_depth.saturating_add(1),
                    max_depth,
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
        if size < min_size_bytes {
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
    fn root_only_scan_depth_scans_only_top_level_files() {
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
            scan_depth: ScanDepth::RootOnly,
            min_size_bytes: 1,
        }];

        let index = scan_library_folders(&folders, &opts);
        assert_eq!(index.report.issues.len(), 0);
        assert_eq!(index.tracks.len(), 1);
        assert!(index.tracks[0].path.ends_with("a.ogg"));
    }

    #[test]
    fn recursive_scan_depth_scans_recursively() {
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
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: 1,
        }];

        let index = scan_library_folders(&folders, &opts);
        assert_eq!(index.report.issues.len(), 0);
        assert_eq!(index.tracks.len(), 2);
    }

    #[test]
    fn one_level_scan_depth_scans_root_and_direct_subfolders_only() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().join("music");
        let root_track = root.join("a.ogg");
        let sub1_track = root.join("sub1").join("b.ogg");
        let sub2_track = root.join("sub1").join("sub2").join("c.ogg");
        write_dummy_file(&root_track, 16);
        write_dummy_file(&sub1_track, 16);
        write_dummy_file(&sub2_track, 16);

        let opts = ScanOptions {
            supported_extensions: vec!["ogg".to_string()],
            min_size_bytes: 1,
            allow_name_size_fallback_dedup: true,
            force_canonicalize_fail: false,
        };

        let folders = vec![FolderScanEntry {
            path: root.to_string_lossy().to_string(),
            scan_depth: ScanDepth::OneLevel,
            min_size_bytes: 1,
        }];

        let index = scan_library_folders(&folders, &opts);
        assert_eq!(index.report.issues.len(), 0);
        assert_eq!(
            index.tracks.len(),
            2,
            "expected to include root + depth=1, but not depth=2"
        );
        let joined = index
            .tracks
            .iter()
            .map(|t| t.path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("a.ogg"),
            "missing root track; got:\n{joined}"
        );
        assert!(
            joined.contains("b.ogg"),
            "missing depth-1 track; got:\n{joined}"
        );
        assert!(
            !joined.contains("c.ogg"),
            "unexpected depth-2 track; got:\n{joined}"
        );
    }

    #[test]
    fn per_folder_min_size_bytes_filters_files_per_root() {
        let td = tempfile::tempdir().unwrap();
        let root_a = td.path().join("a");
        let root_b = td.path().join("b");
        let a_small = root_a.join("s.ogg");
        let b_small = root_b.join("s.ogg");
        write_dummy_file(&a_small, 16);
        write_dummy_file(&b_small, 16);

        let opts = ScanOptions {
            supported_extensions: vec!["ogg".to_string()],
            min_size_bytes: 1, // ignored; per-folder min_size_bytes is used
            allow_name_size_fallback_dedup: true,
            force_canonicalize_fail: false,
        };

        let folders = vec![
            FolderScanEntry {
                path: root_a.to_string_lossy().to_string(),
                scan_depth: ScanDepth::RootOnly,
                min_size_bytes: 1,
            },
            FolderScanEntry {
                path: root_b.to_string_lossy().to_string(),
                scan_depth: ScanDepth::RootOnly,
                min_size_bytes: 1000, // filters out 16-byte file
            },
        ];

        let index = scan_library_folders(&folders, &opts);
        assert_eq!(index.report.issues.len(), 0);
        assert_eq!(index.tracks.len(), 1);
        assert!(index.tracks[0].path.ends_with("s.ogg"));
        // Ensure it came from root_a not root_b by checking parent folder.
        let p = index.tracks[0].path.to_string_lossy().to_string();
        assert!(p.contains("\\a\\") || p.contains("/a/"), "got path: {p}");
    }

    #[test]
    fn fingerprint_is_deterministic_for_same_inputs_even_if_order_differs() {
        let opts = ScanOptions {
            supported_extensions: vec!["ogg".to_string(), ".FlAc".to_string()],
            min_size_bytes: 123,
            allow_name_size_fallback_dedup: true,
            force_canonicalize_fail: false,
        };
        let a = FolderScanEntry {
            path: "C:/Music".to_string(),
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: 1,
        };
        let b = FolderScanEntry {
            path: "C:/Music2".to_string(),
            scan_depth: ScanDepth::RootOnly,
            min_size_bytes: 2,
        };

        let fp1 = compute_index_fingerprint(&[a.clone(), b.clone()], &opts);
        let fp2 = compute_index_fingerprint(&[b, a], &opts);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_is_stable_under_extension_normalization() {
        let folders = vec![FolderScanEntry {
            path: "C:/Music".to_string(),
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: 1,
        }];

        let a = ScanOptions {
            supported_extensions: vec![".OGG".to_string(), " mp3 ".to_string(), "ogg".to_string()],
            min_size_bytes: 10,
            allow_name_size_fallback_dedup: false,
            force_canonicalize_fail: false,
        };
        let b = ScanOptions {
            supported_extensions: vec!["mp3".to_string(), "ogg".to_string()],
            min_size_bytes: 10,
            allow_name_size_fallback_dedup: false,
            force_canonicalize_fail: false,
        };

        assert_eq!(
            compute_index_fingerprint(&folders, &a),
            compute_index_fingerprint(&folders, &b),
            "extension list normalization should not affect fingerprint"
        );
    }

    #[test]
    fn fingerprint_changes_when_scan_parameters_change() {
        let folders = vec![FolderScanEntry {
            path: "C:/Music".to_string(),
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: 1,
        }];

        let base = ScanOptions {
            supported_extensions: vec!["ogg".to_string()],
            min_size_bytes: 10,
            allow_name_size_fallback_dedup: false,
            force_canonicalize_fail: false,
        };
        let mut changed = base.clone();
        changed.min_size_bytes = 11;

        assert_ne!(
            compute_index_fingerprint(&folders, &base),
            compute_index_fingerprint(&folders, &changed),
            "min_size_bytes should participate in fingerprint"
        );
    }

    #[test]
    fn fingerprint_changes_when_folder_entry_changes() {
        let opts = ScanOptions {
            supported_extensions: vec!["ogg".to_string()],
            min_size_bytes: 10,
            allow_name_size_fallback_dedup: false,
            force_canonicalize_fail: false,
        };

        let a = FolderScanEntry {
            path: "C:/Music".to_string(),
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: 1,
        };
        let mut b = a.clone();
        b.scan_depth = ScanDepth::RootOnly;

        assert_ne!(
            compute_index_fingerprint(&[a], &opts),
            compute_index_fingerprint(&[b], &opts),
            "scan_depth should participate in fingerprint"
        );
    }

    #[test]
    #[cfg(windows)]
    fn fingerprint_normalizes_windows_paths_for_stability() {
        let opts = ScanOptions {
            supported_extensions: vec!["ogg".to_string()],
            min_size_bytes: 0,
            allow_name_size_fallback_dedup: false,
            force_canonicalize_fail: false,
        };

        let a = FolderScanEntry {
            path: " C:/Music ".to_string(),
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: 1,
        };
        let b = FolderScanEntry {
            path: "c:\\music".to_string(),
            scan_depth: ScanDepth::Recursive,
            min_size_bytes: 1,
        };

        assert_eq!(
            compute_index_fingerprint(&[a], &opts),
            compute_index_fingerprint(&[b], &opts),
            "fingerprint should be stable across slash/case/whitespace variations on Windows"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn normalize_fingerprint_path_does_not_change_separators_on_non_windows() {
        assert_eq!(normalize_fingerprint_path(" /a/b/c "), "/a/b/c");
        assert_eq!(normalize_fingerprint_path("/a\\b"), "/a\\b");
    }
}
