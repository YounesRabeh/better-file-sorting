// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Stores and loads occurrence manifests describing ingested content and its origins.
//! Content hashes identify objects; occurrence identities retain distinct source events.
//! Loading validates bounded records, reconciles duplicates and tolerates interrupted
//! final journal lines. Optional remote provenance preserves compatibility with old data.

use crate::{app::config::SecurityMode, repository::security, repository::storage::valid_hash};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<NestedProvenance>,
    #[serde(default)]
    pub detection: crate::detection::classifier::Detection,
    #[serde(default)]
    pub mismatch: crate::detection::mismatch::Evaluation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional remote source context; absent in legacy and local records.
    pub provenance: Option<crate::collection::acquisition::Candidate>,
    /// Old manifests conservatively retain archive-sample safeguards.
    #[serde(default)]
    pub input_mode: crate::app::config::InputMode,
    pub occurrence_id: String,
    /// Content digest used to verify integrity and identify stored objects.
    pub sha256: String,
    pub size: u64,
    pub file_type: String,
    pub original_name: String,
    pub original_entry_name: String,
    pub original_stem: String,
    pub original_extension: String,
    pub source_archive: String,
    pub source_path: String,
    pub entry_index: usize,
    pub encrypted: bool,
    pub ingest_date: String,
    pub run_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NestedProvenance {
    pub schema_version: u32,
    pub parent_sha256: String,
    pub parent_occurrence_id: Option<String>,
    pub root_source: String,
    pub root_input_hash: String,
    pub archive_depth: usize,
    pub entry_chain: Vec<usize>,
}
/// Hash bytes as lowercase hexadecimal SHA-256.
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Occurrence {
    /// Validate identity and file-type fields loaded from a manifest.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_hash(&self.sha256) && valid_hash(&self.occurrence_id),
            "invalid manifest identity"
        );
        ensure!(
            [
                "pe", "elf", "macho", "pdf", "zip", "text", "other", "unknown"
            ]
            .contains(&self.file_type.as_str()),
            "invalid manifest file type"
        );
        if let Some(n) = &self.nested {
            ensure!(
                n.schema_version == 1
                    && valid_hash(&n.parent_sha256)
                    && valid_hash(&n.root_input_hash)
                    && n.parent_occurrence_id
                        .as_ref()
                        .is_none_or(|id| valid_hash(id))
                    && !n.entry_chain.is_empty()
                    && n.archive_depth.checked_add(1) == Some(n.entry_chain.len()),
                "invalid nested provenance"
            );
        }
        Ok(())
    }
}
/// Append one serialized JSON record and synchronize the file.
pub fn append(writer: &mut fs::File, record: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.sync_data()?;
    Ok(())
}
/// Load, validate, deduplicate, and deterministically order manifest records.
pub fn load(root: &Path) -> Result<Vec<Occurrence>> {
    crate::ingest::discovery::check_path(&root.join("manifests"))?;
    let mut paths: Vec<_> = fs::read_dir(root.join("manifests"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    let mut records = BTreeMap::new();
    for path in paths {
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        security::validate_file(&path, SecurityMode::Standard)?;
        let mut reader = BufReader::new(fs::File::open(path)?);
        loop {
            let mut line = Vec::new();
            // Bound each untrusted metadata record independently.
            let count =
                std::io::Read::take(&mut reader, 1024 * 1024 + 1).read_until(b'\n', &mut line)?;
            if count == 0 {
                break;
            }
            ensure!(count <= 1024 * 1024, "manifest record exceeds 1 MiB");
            if !line.ends_with(b"\n") {
                tracing::warn!("ignoring interrupted final manifest record");
                break;
            }
            let record: Occurrence = serde_json::from_slice(&line)?;
            record.validate()?;
            if let Some(previous) = records.get(&record.occurrence_id) {
                let previous: &Occurrence = previous;
                ensure!(
                    previous.sha256 == record.sha256 && previous.size == record.size,
                    "conflicting occurrence metadata"
                );
            } else {
                records.insert(record.occurrence_id.clone(), record);
            }
        }
    }
    let analysis = root.join("reports/classification.jsonl");
    crate::ingest::discovery::check_path(&analysis)?;
    if analysis.try_exists()? {
        security::validate_file(&analysis, SecurityMode::Standard)?;
        let mut reader = BufReader::new(fs::File::open(&analysis)?);
        loop {
            let mut line = Vec::new();
            let count =
                std::io::Read::take(&mut reader, 1024 * 1024 + 1).read_until(b'\n', &mut line)?;
            if count == 0 {
                break;
            }
            ensure!(
                count <= 1024 * 1024 && line.ends_with(b"\n"),
                "invalid analysis record"
            );
            let updated: Occurrence = serde_json::from_slice(&line)?;
            updated.validate()?;
            let original = records
                .get_mut(&updated.occurrence_id)
                .ok_or_else(|| anyhow::anyhow!("analysis references unknown occurrence"))?;
            ensure!(
                original.sha256 == updated.sha256 && original.size == updated.size,
                "analysis identity mismatch"
            );
            original.detection = updated.detection;
            original.mismatch = updated.mismatch;
        }
    }
    let mut result: Vec<Occurrence> = records.into_values().collect();
    result.sort_by(|a, b| {
        (&a.source_path, a.entry_index, &a.occurrence_id).cmp(&(
            &b.source_path,
            b.entry_index,
            &b.occurrence_id,
        ))
    });
    Ok(result)
}

/// Explicitly refresh detection metadata without changing payloads or original journals.
pub fn analyze(
    repo: &crate::repository::storage::Repository,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<usize> {
    reclassify(repo, cancelled, false)
}

pub fn reclassify(
    repo: &crate::repository::storage::Repository,
    cancelled: &std::sync::atomic::AtomicBool,
    resume: bool,
) -> Result<usize> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    ensure!(
        repo.mode() == SecurityMode::Standard,
        "analysis requires a standard repository; sealed payloads are never unlocked automatically"
    );
    let mut records = load(repo.root())?;
    let mut cache = BTreeMap::new();
    let checkpoint_dir = repo.root().join("reports/reclassification");
    security::private_dir(&checkpoint_dir)?;
    for record in &mut records {
        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
        if !cache.contains_key(&record.sha256) {
            let path = repo.object(&record.sha256)?;
            security::validate_file(&path, SecurityMode::Standard)?;
            let mut file = fs::File::open(path)?;
            ensure!(
                file.metadata()?.len() == record.size,
                "manifest/object size mismatch"
            );
            let checkpoint = checkpoint_dir.join(format!(
                "{}-{}.json",
                record.sha256,
                crate::detection::classifier::DETECTOR_VERSION
            ));
            crate::ingest::discovery::check_path(&checkpoint)?;
            let hash = crate::ingest::nested::hash_file(&mut file, cancelled)?;
            ensure!(
                hash == record.sha256,
                "reclassification object checksum mismatch"
            );
            if resume && checkpoint.try_exists()? {
                security::validate_file(&checkpoint, SecurityMode::Standard)?;
                ensure!(
                    fs::metadata(&checkpoint)?.len() <= 65536,
                    "oversized classification checkpoint"
                );
                let detection: crate::detection::classifier::Detection =
                    serde_json::from_reader(fs::File::open(&checkpoint)?)?;
                ensure!(
                    detection.detector_version == crate::detection::classifier::DETECTOR_VERSION,
                    "stale classification checkpoint"
                );
                cache.insert(record.sha256.clone(), detection);
            } else {
                let mut prefix = Vec::with_capacity(crate::detection::classifier::PREFIX_LIMIT);
                Read::take(&mut file, crate::detection::classifier::PREFIX_LIMIT as u64)
                    .read_to_end(&mut prefix)?;
                cache.insert(
                    record.sha256.clone(),
                    crate::detection::classifier::detect_reader(&prefix, &mut file),
                );
            }
            let mut saved = tempfile::NamedTempFile::new_in(&checkpoint_dir)?;
            serde_json::to_writer(&mut saved, &cache[&record.sha256])?;
            saved.as_file().sync_all()?;
            saved.persist(&checkpoint)?;
            fs::File::open(&checkpoint_dir)?.sync_all()?;
        }
        record.detection = cache[&record.sha256].clone();
        record.mismatch =
            crate::detection::mismatch::evaluate(&record.original_entry_name, &record.detection);
    }
    let directory = repo.root().join("reports");
    let target = directory.join("classification.jsonl");
    crate::ingest::discovery::check_path(&target)?;
    if target.try_exists()? {
        security::validate_file(&target, SecurityMode::Standard)?;
    }
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    for record in &records {
        serde_json::to_writer(&mut file, record)?;
        file.write_all(b"\n")?;
    }
    file.as_file().sync_all()?;
    ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
    file.persist(target)?;
    fs::File::open(directory)?.sync_all()?;
    let current = repo.root().join("views/CURRENT");
    crate::ingest::discovery::check_path(&current)?;
    let rules = if current.try_exists()? {
        security::validate_file(&current, SecurityMode::Standard)?;
        ensure!(
            fs::metadata(&current)?.len() <= 4096,
            "invalid current view pointer"
        );
        let generation = fs::read_to_string(&current)?;
        let generation = generation.trim();
        ensure!(
            generation.starts_with("view-")
                && !generation.contains(['/', '\\'])
                && !generation.contains(".."),
            "invalid current view pointer"
        );
        let policy = repo
            .root()
            .join("views")
            .join(generation)
            .join("rules.json");
        crate::ingest::discovery::check_path(&policy)?;
        if policy.try_exists()? {
            security::validate_file(&policy, SecurityMode::Standard)?;
            ensure!(
                fs::metadata(&policy)?.len() <= 1024 * 1024,
                "view rules too large"
            );
            serde_json::from_reader(fs::File::open(policy)?)?
        } else {
            crate::organization::rules::Rules::load_or_preset(None, None)?
        }
    } else {
        crate::organization::rules::Rules::load_or_preset(None, None)?
    };
    crate::organization::organizer::organize(repo, &rules, &rules.plan(&records)?)?;
    Ok(records.len())
}
