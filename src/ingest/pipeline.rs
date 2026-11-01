// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Coordinates discovery, extraction, classification, storage, metadata and organization.
//! Ingest uses bounded workers and durable journals; preview runs the same planning
//! logic in disposable storage. ZIP publication requires successful processing, and
//! remote inputs retain stable source identities across different staging locations.

use crate::{
    app::config::{Config, InputMode, SecurityMode},
    detection::classifier,
    ingest::discovery,
    ingest::extraction,
    organization::rules::{FilenamePolicy, Planned, Rules},
    output::archive,
    output::report,
    repository::metadata::{self, Occurrence},
    repository::security,
    repository::storage::Repository,
};
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
#[derive(Serialize)]
pub struct Failure {
    pub source_path: String,
    pub entry_index: Option<usize>,
    pub error: String,
}
#[derive(Serialize)]
pub struct ZipSummary {
    pub archives: Vec<ZipArchiveSummary>,
    pub payload_bytes: u64,
    pub output_bytes: u64,
    pub saved_bytes: i128,
    pub savings_percent: Option<f64>,
    pub ingest_seconds: f64,
}
#[derive(Serialize)]
pub struct ZipArchiveSummary {
    pub output: std::path::PathBuf,
    pub payload_bytes: u64,
    pub output_bytes: u64,
}
#[derive(Serialize)]
pub struct RunReport {
    pub run_id: String,
    pub accepted: usize,
    pub skipped_files: usize,
    pub reused_occurrences: usize,
    pub failures: usize,
    pub interrupted: bool,
    pub elapsed_seconds: f64,
    pub bytes_per_second: f64,
    pub summary: report::Summary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_failure: Option<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip: Option<ZipSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nested: Option<crate::ingest::nested::TreeReport>,
}
struct Journal {
    manifest: File,
    failures_file: File,
    accepted: usize,
    skipped_files: usize,
    reused_occurrences: usize,
    failures: usize,
    bytes: u64,
    first_failure: Option<Failure>,
}
impl Journal {
    /// Record one archive or entry failure in the run report.
    fn failure(&mut self, path: &Path, index: Option<usize>, error: &anyhow::Error) -> Result<()> {
        let failure = Failure {
            source_path: path.to_string_lossy().into_owned(),
            entry_index: index,
            error: error.to_string(),
        };
        metadata::append(&mut self.failures_file, &failure)?;
        if self.first_failure.is_none() {
            self.first_failure = Some(failure);
        }
        self.failures += 1;
        Ok(())
    }
}
/// Create and synchronize a unique JSONL journal file.
fn create_journal_file(root: &Path, directory: &str, run_id: &str) -> Result<File> {
    let file = tempfile::NamedTempFile::new_in(root.join(directory))?;
    let file = file.persist_noclobber(root.join(directory).join(format!("{run_id}.jsonl")))?;
    File::open(root.join(directory))?.sync_all()?;
    Ok(file)
}

/// Sort in disposable private storage; publish only a complete ZIP without overwriting.
fn ingest_zip(config: &Config, cancelled: &AtomicBool, rules: &Rules) -> Result<RunReport> {
    let started = Instant::now();
    discovery::check_path(&config.output)?;
    ensure!(!config.output.try_exists()?, "ZIP output already exists");
    let password = output_password(config)?;
    let parent = config
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let inputs = discovery::for_ingest(config)?;
    ensure!(!inputs.is_empty(), "no matching input files found");
    let temporary =
        security::private_tempdir(config.staging_dir.as_deref().unwrap_or(parent), ".bfs-zip-")?;
    let mut staging = config.clone();
    staging.output = temporary.path().join("repository");
    staging.zip = false;
    staging.zip_dirs.clear();
    staging.zip_each_dir = false;
    staging.output_password_file = None;
    // Only this new disposable repository is readable; existing sealed storage stays sealed.
    // shortcut: private plaintext staging can survive SIGKILL; use encrypted scratch storage for crash confidentiality.
    staging.security.mode = crate::app::config::SecurityMode::Standard;
    // Discover before staging exists, so sources cannot include temporary payloads.
    staging.inputs = inputs;
    let mut report = ingest_repository(&staging, cancelled, rules)?;
    require_success(&report, "ZIP not published")?;
    let repo = Repository::existing(&staging.output)?;
    let records = metadata::load(repo.root())?;
    let plan = rules.plan(&records)?;
    let exports = archive::plan_exports(&plan, config, rules)?;
    let sizes: BTreeMap<_, _> = records.iter().map(|r| (&r.occurrence_id, r.size)).collect();
    let payload_bytes =
        exports
            .iter()
            .flat_map(|e| &e.entries)
            .try_fold(0u64, |total, entry| {
                total
                    .checked_add(sizes[&entry.occurrence_id])
                    .context("ZIP payload size overflow")
            })?;
    archive::publish_exports(
        &repo,
        &exports,
        config,
        password.as_deref(),
        cancelled,
        &records,
    )?;
    let archives = exports
        .iter()
        .map(|export| {
            Ok(ZipArchiveSummary {
                output: export.output.clone(),
                payload_bytes: export.entries.iter().try_fold(0u64, |n, e| {
                    n.checked_add(sizes[&e.occurrence_id])
                        .context("ZIP payload size overflow")
                })?,
                output_bytes: std::fs::metadata(&export.output)?.len(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let output_bytes = archives.iter().try_fold(0u64, |n, a| {
        n.checked_add(a.output_bytes)
            .context("ZIP output size overflow")
    })?;
    report.zip = Some(ZipSummary {
        archives,
        payload_bytes,
        output_bytes,
        saved_bytes: i128::from(payload_bytes) - i128::from(output_bytes),
        savings_percent: (payload_bytes > 0)
            .then(|| (1.0 - output_bytes as f64 / payload_bytes as f64) * 100.0),
        ingest_seconds: report.elapsed_seconds,
    });
    drop(repo);
    temporary
        .close()
        .context("ZIP published but temporary storage cleanup failed")?;
    report.elapsed_seconds = started.elapsed().as_secs_f64();
    report.bytes_per_second =
        report.summary.logical_bytes as f64 / report.elapsed_seconds.max(f64::EPSILON);
    Ok(report)
}

fn require_success(report: &RunReport, operation: &str) -> Result<()> {
    if let Some(failure) = &report.first_failure {
        let entry = failure
            .entry_index
            .map(|i| format!(" entry {i}"))
            .unwrap_or_default();
        anyhow::bail!(
            "{operation}: {}{entry}: {} ({} failure(s))",
            failure.source_path,
            failure.error,
            report.failures
        );
    }
    ensure!(
        !report.interrupted && report.failures == 0,
        "{operation}: interrupted or failed"
    );
    Ok(())
}

pub(crate) fn output_password(config: &Config) -> Result<Option<String>> {
    let mut passwords = security::passwords(config.output_password_file.as_deref())?;
    ensure!(
        config.output_password_file.is_none() || passwords.len() == 1,
        "output password file must contain exactly one nonempty password"
    );
    Ok(passwords.pop())
}

fn ingest_rules(config: &Config) -> Result<Rules> {
    let rules = Rules::load_or_preset(config.rules.as_deref(), config.preset)?;
    ensure!(
        config.input_mode == InputMode::Files
            || rules.organization.filename_policy == FilenamePolicy::Samples,
        "regular filename policy is forbidden for archive inputs"
    );
    Ok(rules)
}

#[derive(Serialize)]
pub struct PreviewEntry {
    pub source_path: String,
    pub original_entry_name: String,
    #[serde(flatten)]
    pub planned: Planned,
}

#[derive(Serialize)]
pub struct PreviewSummary {
    pub occurrences: usize,
    pub unique_files: usize,
    pub duplicate_occurrences: usize,
}

#[derive(Serialize)]
pub struct Preview {
    pub plan: Vec<PreviewEntry>,
    pub summary: PreviewSummary,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub archives: Vec<archive::ZipExport>,
}

/// Compute real destinations in sealed disposable storage without changing the output.
pub fn preview(config: &Config, cancelled: &AtomicBool) -> Result<Preview> {
    config.validate()?;
    ensure!(
        !config.resume || !config.zip_enabled(),
        "--resume requires a persistent repository: ingest --resume --output <REPOSITORY>, then export ZIPs separately"
    );
    let rules = ingest_rules(config)?;
    if config.zip_enabled() {
        ensure!(!config.output.try_exists()?, "ZIP output already exists");
        output_password(config)?;
    }
    let existing = if !config.zip_enabled() && config.output.try_exists()? {
        let repo = Repository::existing(&config.output)?;
        ensure!(
            repo.mode() == config.security.mode,
            "repository security mode cannot be changed automatically"
        );
        Some(repo)
    } else {
        None
    };
    let mut records = existing
        .as_ref()
        .map(|repo| metadata::load(repo.root()))
        .transpose()?
        .unwrap_or_default();
    let inputs = discovery::for_ingest(config)?;
    ensure!(!inputs.is_empty(), "no matching input files found");
    let default_parent = std::env::temp_dir();
    let temporary = security::private_tempdir(
        config.staging_dir.as_deref().unwrap_or(&default_parent),
        ".bfs-preview-",
    )?;
    let mut staging = config.clone();
    staging.output = temporary.path().join("repository");
    staging.inputs = inputs;
    staging.zip = false;
    staging.zip_dirs.clear();
    staging.zip_each_dir = false;
    staging.output_password_file = None;
    staging.security.mode = SecurityMode::Sealed;
    staging.resume = false;
    let report = ingest_repository(&staging, cancelled, &rules)?;
    require_success(&report, "preview failed")?;
    records.extend(metadata::load(&staging.output)?);
    let mut merged = BTreeMap::new();
    for row in records {
        merged.entry(row.occurrence_id.clone()).or_insert(row);
    }
    let records: Vec<_> = merged.into_values().collect();
    let mut plan = rules.plan(&records)?;
    let archives = if config.zip_enabled() {
        archive::plan_exports(&plan, config, &rules)?
    } else {
        vec![]
    };
    if config.zip_enabled() {
        let selected: BTreeSet<_> = archives
            .iter()
            .flat_map(|e| e.entries.iter().map(|p| &p.occurrence_id))
            .collect();
        plan.retain(|p| selected.contains(&p.occurrence_id));
    }
    let sources: BTreeMap<_, _> = records.iter().map(|r| (&r.occurrence_id, r)).collect();
    let unique_files = records
        .iter()
        .map(|r| &r.sha256)
        .collect::<BTreeSet<_>>()
        .len();
    Ok(Preview {
        archives,
        plan: plan
            .into_iter()
            .map(|planned| {
                let source = sources[&planned.occurrence_id];
                PreviewEntry {
                    source_path: source.source_path.clone(),
                    original_entry_name: source.original_entry_name.clone(),
                    planned,
                }
            })
            .collect(),
        summary: PreviewSummary {
            occurrences: records.len(),
            unique_files,
            duplicate_occurrences: records.len() - unique_files,
        },
    })
}
/// Ingest discovered archives with bounded parallel workers and durable metadata.
pub fn ingest(config: &Config, cancelled: &AtomicBool) -> Result<RunReport> {
    config.validate()?;
    ensure!(
        !config.resume || !config.zip_enabled(),
        "--resume requires a persistent repository: ingest --resume --output <REPOSITORY>, then export ZIPs separately"
    );
    let rules = ingest_rules(config)?;
    if config.zip_enabled() {
        return ingest_zip(config, cancelled, &rules);
    }
    ingest_repository(config, cancelled, &rules)
}

fn ingest_repository(config: &Config, cancelled: &AtomicBool, rules: &Rules) -> Result<RunReport> {
    let passwords = security::passwords(config.password_file.as_deref())?;
    let paths = discovery::for_ingest(config)?;
    ensure!(!paths.is_empty(), "no matching input files found");
    let repo = Repository::open(&config.output, config.security.mode)?;
    // Unchanged completed inputs already proved decryption; passwords affect retries only.
    // Never persist password contents or a dictionary-testable password digest.
    let checkpoints = config.resume;
    let checkpoint_dir = repo.root().join("operations/ingest");
    if checkpoints {
        security::private_dir(&repo.root().join("operations"))?;
        security::private_dir(&checkpoint_dir)?;
    }
    let existing: BTreeMap<_, _> = metadata::load(repo.root())?
        .into_iter()
        .map(|record| (record.occurrence_id.clone(), record))
        .collect();
    let fingerprint = metadata::digest(
        format!(
            "ingest-checkpoint-v1/{}/{}/{:?}/{:?}/{:?}",
            env!("CARGO_PKG_VERSION"),
            classifier::DETECTOR_VERSION,
            config.security,
            config.input_mode,
            config.limits
        )
        .as_bytes(),
    );
    let marker = security::private_tempdir(&repo.staging(), "run-")?;
    let run_id = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
        marker
            .path()
            .file_name()
            .expect("run name")
            .to_string_lossy()
    );
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let journal = Mutex::new(Journal {
        manifest: create_journal_file(repo.root(), "manifests", &run_id)?,
        failures_file: create_journal_file(repo.root(), "reports", &run_id)?,
        accepted: 0,
        skipped_files: 0,
        reused_occurrences: 0,
        failures: 0,
        bytes: 0,
        first_failure: None,
    });
    let progress =
        crate::app::presentation::Progress::new(Some(paths.len() as u64), "Organizing inputs");
    let started = Instant::now();
    let nested_cache = crate::ingest::nested::Cache::with_records(existing.values());
    let nested_report = Mutex::new(crate::ingest::nested::TreeReport {
        schema_version: 1,
        ..Default::default()
    });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.workers)
        .build()?;
    pool.install(|| {
        paths.par_iter().try_for_each(|path| -> Result<()> {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(());
            }
            let work = || -> Result<()> {
                if config.archive.nested_extraction {
                    let mut nested_manifest = create_journal_file(
                        repo.root(),
                        "manifests",
                        &format!(
                            "{run_id}-{}",
                            metadata::digest(path.as_os_str().as_encoded_bytes())
                        ),
                    )?;
                    let result = crate::ingest::nested::ingest_root(
                        &repo,
                        path,
                        config,
                        cancelled,
                        &nested_cache,
                        &mut nested_manifest,
                        &run_id,
                    )?;
                    let mut journal = journal
                        .lock()
                        .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
                    journal.accepted += result.accepted;
                    journal.reused_occurrences += result.reused_occurrences;
                    journal.failures += result.failures;
                    journal.bytes += result.metrics.decompressed_bytes;
                    if result.failures > 0 {
                        journal.first_failure = Some(Failure {
                            source_path: path.to_string_lossy().into_owned(),
                            entry_index: None,
                            error: result
                                .outcomes
                                .iter()
                                .filter_map(|o| o.diagnostic.as_deref())
                                .collect::<Vec<_>>()
                                .join("; "),
                        });
                    }
                    nested_report
                        .lock()
                        .map_err(|_| anyhow::anyhow!("nested report lock poisoned"))?
                        .merge(result);
                    return Ok(());
                }
                let fingerprint = metadata::digest(&serde_json::to_vec(&(
                    &fingerprint,
                    config.remote_inputs.get(path),
                ))?);
                let input_hash = if checkpoints {
                    Some(hash_input(path, cancelled)?)
                } else {
                    None
                };
                let checkpoint_path = checkpoint_dir.join(format!(
                    "{}.json",
                    metadata::digest(path.as_os_str().as_encoded_bytes())
                ));
                if let Some(hash) = &input_hash
                    && let Some(reused) = checkpoint_reusable(
                        &checkpoint_path,
                        hash,
                        &fingerprint,
                        &repo,
                        &existing,
                        cancelled,
                    )?
                {
                    let mut journal = journal
                        .lock()
                        .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
                    journal.skipped_files += 1;
                    journal.reused_occurrences += reused;
                    return Ok(());
                }
                let mut completed_records = Vec::new();
                let mut complete = true;
                let mut zip = if config.input_mode == crate::app::config::InputMode::Archives {
                    Some(archive::open(path, &config.limits)?)
                } else {
                    None
                };
                let mut total = 0;
                for index in 0..zip.as_ref().map_or(1, |z| z.len()) {
                    if cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let (encrypted, original, result) = if let Some(zip) = &mut zip {
                        let data = zip.by_index_data(index)?;
                        if data.is_dir() {
                            continue;
                        }
                        let encrypted = data.encrypted();
                        let original = data.name()?.into_owned();
                        let result = extraction::extract(
                            zip,
                            index,
                            &passwords,
                            marker.path(),
                            &config.limits,
                            &mut total,
                            cancelled,
                        );
                        (encrypted, original, result)
                    } else {
                        (
                            false,
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                            extraction::extract_file(
                                path,
                                marker.path(),
                                &config.limits,
                                cancelled,
                            ),
                        )
                    };
                    match result {
                        Ok(mut item) => {
                            let file_type = classifier::classify(&item.prefix).to_string();
                            let detection =
                                classifier::detect_reader(&item.prefix, item.file.as_file_mut());
                            let mismatch =
                                crate::detection::mismatch::evaluate(&original, &detection);
                            repo.commit(item.file, &item.sha256, item.size)?;
                            let basename = original
                                .rsplit(['/', '\\'])
                                .next()
                                .unwrap_or("")
                                .to_string();
                            let name_path = Path::new(&basename);
                            let source_path =
                                path.to_str().map(str::to_owned).unwrap_or_else(|| {
                                    let hex: String = path
                                        .as_os_str()
                                        .as_encoded_bytes()
                                        .iter()
                                        .map(|b| format!("{b:02x}"))
                                        .collect();
                                    format!("encoded-path:{hex}")
                                });
                            let identity_source = config
                                .remote_inputs
                                .get(path)
                                .map(|c| c.source_id.as_str())
                                .unwrap_or(&source_path);
                            let id = metadata::digest(&serde_json::to_vec(&(
                                identity_source,
                                index,
                                &item.sha256,
                            ))?);
                            let record = Occurrence {
                                nested: None,
                                detection,
                                mismatch,
                                provenance: config.remote_inputs.get(path).cloned(),
                                input_mode: config.input_mode,
                                occurrence_id: id,
                                sha256: item.sha256,
                                size: item.size,
                                file_type,
                                original_stem: name_path
                                    .file_stem()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                original_extension: name_path
                                    .extension()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                original_name: basename,
                                original_entry_name: original,
                                source_archive: path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                source_path,
                                entry_index: index,
                                encrypted,
                                ingest_date: date.clone(),
                                run_id: run_id.clone(),
                            };
                            let mut journal = journal
                                .lock()
                                .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
                            metadata::append(&mut journal.manifest, &record)?;
                            completed_records.push(record);
                            journal.accepted += 1;
                            journal.bytes += item.size;
                            progress.0.set_message(format!(
                                "{} items; {} failed; {} processed",
                                journal.accepted,
                                journal.failures,
                                crate::app::presentation::bytes(journal.bytes)
                            ));
                        }
                        Err(error) => {
                            complete = false;
                            journal
                                .lock()
                                .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?
                                .failure(path, zip.as_ref().map(|_| index), &error)?;
                        }
                    }
                }
                if complete
                    && !cancelled.load(Ordering::Relaxed)
                    && let Some(input_hash) = input_hash
                {
                    // Recheck after processing so changed inputs never receive a completed checkpoint.
                    ensure!(
                        hash_input(path, cancelled)? == input_hash,
                        "input changed during ingestion"
                    );
                    let checkpoint = IngestCheckpoint {
                        schema_version: 1,
                        input_hash,
                        fingerprint: fingerprint.clone(),
                        records: completed_records,
                    };
                    let mut file = tempfile::NamedTempFile::new_in(&checkpoint_dir)?;
                    serde_json::to_writer(&mut file, &checkpoint)?;
                    file.as_file().sync_all()?;
                    discovery::check_path(&checkpoint_path)?;
                    file.persist(&checkpoint_path)?;
                    File::open(&checkpoint_dir)?.sync_all()?;
                }
                Ok(())
            };
            if let Err(error) = work() {
                journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?
                    .failure(path, None, &error)?;
            }
            {
                let journal = journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
                progress.0.set_message(format!(
                    "{} items; {} failed; {} processed",
                    journal.accepted,
                    journal.failures,
                    crate::app::presentation::bytes(journal.bytes)
                ));
            }
            progress.0.inc(1);
            Ok(())
        })
    })?;
    progress.0.finish_and_clear();
    let journal = journal
        .into_inner()
        .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
    let records = metadata::load(repo.root())?;
    let plan = rules.plan(&records)?;
    crate::organization::organizer::organize(&repo, rules, &plan)?;
    let elapsed = started.elapsed().as_secs_f64();
    let report = RunReport {
        run_id: run_id.clone(),
        accepted: journal.accepted,
        skipped_files: journal.skipped_files,
        reused_occurrences: journal.reused_occurrences,
        failures: journal.failures,
        interrupted: cancelled.load(Ordering::Relaxed),
        elapsed_seconds: elapsed,
        bytes_per_second: journal.bytes as f64 / elapsed.max(f64::EPSILON),
        summary: report::summary(&repo, &records)?,
        first_failure: journal.first_failure,
        zip: None,
        nested: config
            .archive
            .nested_extraction
            .then(|| nested_report.into_inner().expect("nested report lock")),
    };
    let mut file = tempfile::NamedTempFile::new_in(repo.root().join("reports"))?;
    serde_json::to_writer_pretty(&mut file, &report)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(repo.root().join("reports").join(format!("{run_id}.json")))
        .context("cannot publish report")?;
    Ok(report)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IngestCheckpoint {
    schema_version: u32,
    input_hash: String,
    fingerprint: String,
    records: Vec<Occurrence>,
}

/// Streaming verification deliberately rereads inputs and readable canonical objects.
fn hash_input(path: &Path, cancelled: &AtomicBool) -> Result<String> {
    discovery::check_path(path)?;
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn checkpoint_reusable(
    path: &Path,
    input_hash: &str,
    fingerprint: &str,
    repo: &Repository,
    records: &BTreeMap<String, Occurrence>,
    cancelled: &AtomicBool,
) -> Result<Option<usize>> {
    discovery::check_path(path)?;
    if !path.try_exists()? {
        return Ok(None);
    }
    security::validate_file(path, SecurityMode::Standard)?;
    let checkpoint: IngestCheckpoint = match serde_json::from_reader(File::open(path)?) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    ensure!(
        checkpoint.schema_version == 1,
        "unsupported ingestion checkpoint version; use a fresh repository"
    );
    if checkpoint.input_hash != input_hash || checkpoint.fingerprint != fingerprint {
        return Ok(None);
    }
    let mut verified_objects = BTreeSet::new();
    for record in &checkpoint.records {
        record.validate()?;
        let Some(current) = records.get(&record.occurrence_id) else {
            return Ok(None);
        };
        let mut expected = record.clone();
        expected.run_id = current.run_id.clone();
        expected.ingest_date = current.ingest_date.clone();
        if serde_json::to_value(current)? != serde_json::to_value(&expected)? {
            return Ok(None);
        }
        let object = repo.object(&record.sha256)?;
        if !object.try_exists()? {
            return Ok(None);
        }
        security::validate_file(&object, repo.mode())?;
        if std::fs::metadata(&object)?.len() != record.size {
            return Ok(None);
        }
        if repo.mode() == SecurityMode::Standard && verified_objects.insert(&record.sha256) {
            ensure!(
                hash_input(&object, cancelled)? == record.sha256,
                "canonical object failed integrity verification"
            );
        }
    }
    Ok(Some(checkpoint.records.len()))
}
