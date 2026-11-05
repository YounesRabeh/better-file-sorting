//! Build independent repositories through private staging and verified object copies.
use super::{
    metadata::{self, Occurrence},
    security,
    storage::Repository,
};
use crate::{app::config::SecurityMode, selection::Filters};
use anyhow::{Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Options {
    pub sources: Vec<PathBuf>,
    pub output: PathBuf,
    pub kind: Kind,
    pub dry_run: bool,
}
pub enum Kind {
    Merge,
    Sample { count: usize, seed: u64 },
    Split { by: SplitBy },
}
pub enum SplitBy {
    Extension,
    Format,
}
#[derive(Debug, Serialize)]
pub struct GroupReport {
    pub name: String,
    pub occurrences: usize,
    pub unique_objects: usize,
    pub estimated_copy_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub requested_occurrences: usize,
    pub occurrences: usize,
    pub unique_objects: usize,
    pub estimated_copy_bytes: u64,
    pub ancestry_additions: usize,
    pub groups: Vec<GroupReport>,
}
fn interrupted(cancelled: &AtomicBool) -> Result<()> {
    ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
    Ok(())
}
/// Rank occurrences by SHA-256 of the domain, big-endian seed and ASCII occurrence ID.
fn rank(seed: u64, record: &Occurrence) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"bfsort-sample-v1\0");
    hash.update(seed.to_be_bytes());
    hash.update(record.occurrence_id.as_bytes());
    hash.finalize().into()
}
fn group(record: &Occurrence, by: &SplitBy) -> String {
    match by {
        SplitBy::Format => record.detection.detected_format.as_str().to_owned(),
        SplitBy::Extension => {
            let extension = record
                .original_extension
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if extension.is_empty() {
                "unknown".to_owned()
            }
            // Encoding all bytes prevents path traversal and case/escape collisions.
            else {
                format!(
                    "ext-{}",
                    extension
                        .bytes()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                )
            }
        }
    }
}
fn object_summary(records: &[Occurrence]) -> Result<(usize, u64)> {
    let mut objects = BTreeMap::new();
    for record in records {
        if let Some(size) = objects.insert(&record.sha256, record.size) {
            ensure!(size == record.size, "conflicting object sizes");
        }
    }
    let bytes = objects.values().try_fold(0u64, |total, size| {
        total
            .checked_add(*size)
            .ok_or_else(|| anyhow::anyhow!("copy size overflow"))
    })?;
    Ok((objects.len(), bytes))
}

pub fn run(options: &Options, filter: &Filters, cancelled: &AtomicBool) -> Result<Report> {
    interrupted(cancelled)?;
    ensure!(
        !options.sources.is_empty(),
        "at least one source repository is required"
    );
    ensure!(
        matches!(options.kind, Kind::Merge) || options.sources.len() == 1,
        "sample and split require one source"
    );
    crate::ingest::discovery::check_path(&options.output)?;
    ensure!(!options.output.try_exists()?, "destination already exists");
    let parent = options
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let parent = parent.canonicalize()?;
    let destination = parent.join(
        options
            .output
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid destination"))?,
    );
    let mut paths = Vec::new();
    for source in &options.sources {
        crate::ingest::discovery::check_path(source)?;
        paths.push(source.canonicalize()?);
    }
    paths.sort();
    ensure!(
        paths.windows(2).all(|pair| pair[0] != pair[1]),
        "aliased source repositories"
    );
    for (index, source) in paths.iter().enumerate() {
        ensure!(
            !destination.starts_with(source) && !source.starts_with(&destination),
            "source and destination overlap"
        );
        ensure!(
            paths[index + 1..]
                .iter()
                .all(|other| !other.starts_with(source)),
            "overlapping source repositories"
        );
    }
    let sources = paths
        .iter()
        .map(|path| Repository::existing(path))
        .collect::<Result<Vec<_>>>()?;
    if !options.dry_run {
        ensure!(
            sources
                .iter()
                .all(|repo| repo.mode() == SecurityMode::Standard),
            "sealed-source transfer is unsupported; payloads are never unlocked automatically"
        );
    }
    let mut records = BTreeMap::<String, Occurrence>::new();
    let mut owners = BTreeMap::new();
    for (index, source) in sources.iter().enumerate() {
        for record in metadata::select(source.root(), filter)? {
            interrupted(cancelled)?;
            security::validate_file(&source.object(&record.sha256)?, source.mode())?;
            ensure!(
                fs::metadata(source.object(&record.sha256)?)?.len() == record.size,
                "manifest/object size mismatch"
            );
            if let Some(previous) = records.get(&record.occurrence_id) {
                ensure!(
                    serde_json::to_value(previous)? == serde_json::to_value(&record)?,
                    "conflicting occurrence identity"
                );
            } else {
                owners.entry(record.sha256.clone()).or_insert(index);
                records.insert(record.occurrence_id.clone(), record);
            }
        }
    }
    let mut selected: Vec<_> = records.into_values().collect();
    if let Kind::Sample { count, seed } = options.kind {
        ensure!(
            count <= selected.len(),
            "sample count exceeds eligible population"
        );
        selected.sort_by_cached_key(|record| (rank(seed, record), record.occurrence_id.clone()));
        selected.truncate(count);
    }
    ensure!(
        selected.iter().all(|record| record.nested.is_none()),
        "derived repositories currently refuse nested occurrences: ancestry closure is not supported"
    );
    selected.sort_by(|a, b| a.occurrence_id.cmp(&b.occurrence_id));
    let (unique_objects, _) = object_summary(&selected)?;
    let mut groups = BTreeMap::<String, Vec<Occurrence>>::new();
    match &options.kind {
        Kind::Split { by } => {
            for record in &selected {
                groups
                    .entry(group(record, by))
                    .or_default()
                    .push(record.clone());
            }
        }
        _ => {
            groups.insert(String::new(), selected.clone());
        }
    }
    let mut group_reports = Vec::new();
    let mut estimated_copy_bytes = 0u64;
    for (name, records) in &groups {
        let (unique_objects, bytes) = object_summary(records)?;
        estimated_copy_bytes = estimated_copy_bytes
            .checked_add(bytes)
            .ok_or_else(|| anyhow::anyhow!("copy size overflow"))?;
        group_reports.push(GroupReport {
            name: name.clone(),
            occurrences: records.len(),
            unique_objects,
            estimated_copy_bytes: bytes,
        });
    }
    let report = Report {
        requested_occurrences: selected.len(),
        occurrences: selected.len(),
        unique_objects,
        estimated_copy_bytes,
        ancestry_additions: 0,
        groups: group_reports,
    };
    interrupted(cancelled)?;
    if options.dry_run {
        return Ok(report);
    }
    let staging = security::private_tempdir(&parent, ".bfsort-derived-")?;
    for (name, records) in groups {
        let target = if name.is_empty() {
            staging.path().join("repository")
        } else {
            staging.path().join(name)
        };
        let repo = Repository::open(&target, SecurityMode::Standard)?;
        repo.with_catalog(|_| Ok(()))?;
        let mut copied = BTreeSet::new();
        let mut manifest = tempfile::NamedTempFile::new_in(repo.root().join("manifests"))?;
        for record in records {
            interrupted(cancelled)?;
            if copied.insert(record.sha256.clone()) {
                let source = &sources[owners[&record.sha256]];
                let mut input = File::open(source.object(&record.sha256)?)?;
                let mut output = tempfile::NamedTempFile::new_in(repo.staging())?;
                let mut hasher = Sha256::new();
                let mut size = 0u64;
                let mut buffer = [0u8; 65536];
                loop {
                    interrupted(cancelled)?;
                    let count = input.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    size = size
                        .checked_add(count as u64)
                        .ok_or_else(|| anyhow::anyhow!("object size overflow"))?;
                    ensure!(size <= record.size, "object exceeds recorded size");
                    hasher.update(&buffer[..count]);
                    output.write_all(&buffer[..count])?;
                }
                ensure!(
                    size == record.size
                        && hasher
                            .finalize()
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>()
                            == record.sha256,
                    "object checksum or size mismatch"
                );
                repo.commit(output, &record.sha256, size)?;
            }
            metadata::publish(&repo, manifest.as_file_mut(), &record)?;
        }
        manifest.as_file().sync_all()?;
        manifest.persist_noclobber(repo.root().join("manifests/derived.jsonl"))?;
        File::open(repo.root().join("manifests"))?.sync_all()?;
    }
    interrupted(cancelled)?;
    let publication = if matches!(options.kind, Kind::Split { .. }) {
        staging.path().to_path_buf()
    } else {
        staging.path().join("repository")
    };
    security::publish_directory(&publication, &destination)?;
    File::open(parent)?.sync_all()?;
    Ok(report)
}
