//! Builds repository summaries from validated occurrence metadata and storage settings.
//! Reports distinguish logical occurrences from unique content so deduplication is
//! visible without reopening payloads. Sealed repositories can therefore be summarized
//! without changing their permissions or reading their contents.

use crate::{
    config::SecurityMode,
    metadata::Occurrence,
    storage::{Repository, valid_hash},
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Serialize)]
pub struct Summary {
    pub occurrences: usize,
    pub unique_samples: usize,
    pub duplicate_occurrences: usize,
    pub stored_objects: usize,
    pub orphan_objects: usize,
    pub logical_bytes: u64,
    pub stored_bytes: u64,
    pub allocated_bytes: u64,
    pub per_category: BTreeMap<String, usize>,
    pub security_mode: SecurityMode,
}
/// Summarize manifest occurrences, canonical objects, and storage usage.
pub fn summary(repo: &Repository, records: &[Occurrence]) -> Result<Summary> {
    let mut hashes = BTreeSet::new();
    let mut categories = BTreeMap::new();
    for r in records {
        *categories.entry(r.file_type.clone()).or_insert(0) += 1;
        if hashes.insert(r.sha256.clone()) {
            let path = repo.object(&r.sha256)?;
            crate::security::validate_file(&path, repo.mode())?;
            ensure!(
                std::fs::metadata(path)?.len() == r.size,
                "manifest/object size mismatch"
            );
        }
    }
    let mut stored = 0;
    let mut allocated = 0;
    let mut stored_objects = 0;
    let mut orphan_objects = 0;
    for entry in walkdir::WalkDir::new(repo.root().join("objects"))
        .follow_links(false)
        .min_depth(1)
    {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "symlink in canonical storage"
        );
        if entry.file_type().is_dir() {
            continue;
        }
        let hash = entry
            .file_name()
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid object name"))?;
        ensure!(
            valid_hash(hash) && entry.path() == repo.object(hash)?,
            "unexpected canonical object path"
        );
        crate::security::validate_file(entry.path(), repo.mode())?;
        let meta = entry.metadata()?;
        stored += meta.len();
        stored_objects += 1;
        if !hashes.contains(hash) {
            orphan_objects += 1;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            allocated += meta.blocks() * 512;
        }
        #[cfg(not(unix))]
        {
            allocated += meta.len();
        }
    }
    Ok(Summary {
        occurrences: records.len(),
        unique_samples: hashes.len(),
        duplicate_occurrences: records.len() - hashes.len(),
        stored_objects,
        orphan_objects,
        logical_bytes: records.iter().map(|r| r.size).sum(),
        stored_bytes: stored,
        allocated_bytes: allocated,
        per_category: categories,
        security_mode: repo.mode(),
    })
}
