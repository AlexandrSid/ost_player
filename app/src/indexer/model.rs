use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TrackId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackEntry {
    pub id: TrackId,
    /// Canonical path when available; otherwise best-effort absolute path.
    pub path: PathBuf,
    /// Relative path to the matched root folder when available (used for deterministic sorting).
    pub rel_path: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexIssueKind {
    MissingFolder,
    PermissionDenied,
    ReadDirFailed,
    MetadataFailed,
    CanonicalizeFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexIssue {
    pub kind: IndexIssueKind,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexReport {
    pub roots_total: usize,
    pub roots_ok: usize,
    pub tracks_total: usize,
    pub files_seen: u64,
    pub skipped_ext: u64,
    pub skipped_small: u64,
    pub deduped: u64,
    pub issues: Vec<IndexIssue>,
    /// Count issues by kind for easy UI summaries.
    pub issue_counts: BTreeMap<IndexIssueKind, u64>,
}

impl IndexReport {
    pub fn record_issue(&mut self, issue: IndexIssue) {
        *self.issue_counts.entry(issue.kind).or_insert(0) += 1;
        self.issues.push(issue);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryIndex {
    pub schema_version: u32,
    pub tracks: Vec<TrackEntry>,
    pub report: IndexReport,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tracks: Vec::new(),
            report: IndexReport::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub supported_extensions: Vec<String>,
    pub min_size_bytes: u64,
    /// If canonicalization fails, optionally deduplicate by (root identity, rel_path, size_bytes).
    pub allow_name_size_fallback_dedup: bool,
}

impl ScanOptions {
    pub fn normalized(&self) -> Self {
        let mut exts = self
            .supported_extensions
            .iter()
            .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        exts.sort();
        exts.dedup();
        Self {
            supported_extensions: exts,
            min_size_bytes: self.min_size_bytes,
            allow_name_size_fallback_dedup: self.allow_name_size_fallback_dedup,
        }
    }
}

