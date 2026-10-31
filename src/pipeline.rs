// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Coordinates discovery, extraction, classification, storage, metadata and organization.
//! Ingest uses bounded workers and durable journals; preview runs the same planning
//! logic in disposable storage. ZIP publication requires successful processing, and
//! remote inputs retain stable source identities across different staging locations.

use crate::{
    archive, classifier,
    config::{Config, InputMode, SecurityMode},
    discovery, extraction,
    metadata::{self, Occurrence},
    report,
    rules::{FilenamePolicy, Planned, Rules},
    security,
    storage::Repository,
};
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
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
    pub failures: usize,
    pub interrupted: bool,
    pub elapsed_seconds: f64,
    pub bytes_per_second: f64,
    pub summary: report::Summary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_failure: Option<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zip: Option<ZipSummary>,
}
struct Journal {
    manifest: File,
    failures_file: File,
    accepted: usize,
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
    staging.security.mode = crate::config::SecurityMode::Standard;
    // Discover before staging exists, so sources cannot include temporary payloads.
    staging.inputs = inputs;
    let mut report = ingest_repository(&staging, cancelled, rules)?;
    require_success(&report, "ZIP not published")?;
    let repo = Repository::existing(&staging.output)?;
    let records = metadata::load(repo.root())?;
    let plan = rules.plan(&records)?;
    let exports = archive::plan_exports(&plan, config)?;
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
    archive::publish_exports(&repo, &exports, config, password.as_deref(), cancelled)?;
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

fn output_password(config: &Config) -> Result<Option<String>> {
    let mut passwords = security::passwords(config.output_password_file.as_deref())?;
    ensure!(
        config.output_password_file.is_none() || passwords.len() == 1,
        "output password file must contain exactly one nonempty password"
    );
    Ok(passwords.pop())
}

fn ingest_rules(config: &Config) -> Result<Rules> {
    let rules = Rules::load(config.rules.as_deref())?;
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
        archive::plan_exports(&plan, config)?
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
        failures: 0,
        bytes: 0,
        first_failure: None,
    });
    let progress = indicatif::ProgressBar::new(paths.len() as u64);
    let started = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.workers)
        .build()?;
    pool.install(|| {
        paths.par_iter().try_for_each(|path| -> Result<()> {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(());
            }
            let work = || -> Result<()> {
                let mut zip = if config.input_mode == crate::config::InputMode::Archives {
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
                        Ok(item) => {
                            let file_type = classifier::classify(&item.prefix).to_string();
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
                            journal.accepted += 1;
                            journal.bytes += item.size;
                        }
                        Err(error) => {
                            journal
                                .lock()
                                .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?
                                .failure(path, zip.as_ref().map(|_| index), &error)?;
                        }
                    }
                }
                Ok(())
            };
            if let Err(error) = work() {
                journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?
                    .failure(path, None, &error)?;
            }
            progress.inc(1);
            Ok(())
        })
    })?;
    progress.finish_and_clear();
    let journal = journal
        .into_inner()
        .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
    let records = metadata::load(repo.root())?;
    let plan = rules.plan(&records)?;
    crate::organizer::organize(&repo, rules, &plan)?;
    let elapsed = started.elapsed().as_secs_f64();
    let report = RunReport {
        run_id: run_id.clone(),
        accepted: journal.accepted,
        failures: journal.failures,
        interrupted: cancelled.load(Ordering::Relaxed),
        elapsed_seconds: elapsed,
        bytes_per_second: journal.bytes as f64 / elapsed.max(f64::EPSILON),
        summary: report::summary(&repo, &records)?,
        first_failure: journal.first_failure,
        zip: None,
    };
    let mut file = tempfile::NamedTempFile::new_in(repo.root().join("reports"))?;
    serde_json::to_writer_pretty(&mut file, &report)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(repo.root().join("reports").join(format!("{run_id}.json")))
        .context("cannot publish report")?;
    Ok(report)
}
