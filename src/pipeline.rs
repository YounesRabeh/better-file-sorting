use crate::{
    archive, classifier,
    config::Config,
    discovery, extraction,
    metadata::{self, Occurrence},
    report, security,
    storage::Repository,
};
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::Serialize;
use std::{
    fs::File,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
#[derive(Serialize)]
struct Failure {
    source_path: String,
    entry_index: Option<usize>,
    error: String,
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
}
struct Journal {
    manifest: File,
    failures_file: File,
    accepted: usize,
    failures: usize,
    bytes: u64,
}
impl Journal {
    /// Record one archive or entry failure in the run report.
    fn failure(&mut self, path: &Path, index: Option<usize>, error: &anyhow::Error) -> Result<()> {
        metadata::append(
            &mut self.failures_file,
            &Failure {
                source_path: path.to_string_lossy().into_owned(),
                entry_index: index,
                error: error.to_string(),
            },
        )?;
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
/// Ingest discovered archives with bounded parallel workers and durable metadata.
pub fn ingest(config: &Config, cancelled: &AtomicBool) -> Result<RunReport> {
    config.validate()?;
    let rules = crate::rules::Rules::load(config.rules.as_deref())?;
    let passwords = security::passwords(config.password_file.as_deref())?;
    let paths = discovery::discover(&config.inputs, config.recursive, &config.output)?;
    ensure!(!paths.is_empty(), "no ZIP archives found");
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
                let mut zip = archive::open(path, &config.limits)?;
                let mut total = 0;
                for index in 0..zip.len() {
                    if cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let data = zip.by_index_data(index)?;
                    if data.is_dir() {
                        continue;
                    }
                    let encrypted = data.encrypted();
                    let original = data.name()?.into_owned();
                    let result = extraction::extract(
                        &mut zip,
                        index,
                        &passwords,
                        marker.path(),
                        &config.limits,
                        &mut total,
                        cancelled,
                    );
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
                            let id = metadata::digest(&serde_json::to_vec(&(
                                &source_path,
                                index,
                                &item.sha256,
                            ))?);
                            let record = Occurrence {
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
                                .failure(path, Some(index), &error)?;
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
    crate::organizer::organize(&repo, &rules, &plan)?;
    let elapsed = started.elapsed().as_secs_f64();
    let report = RunReport {
        run_id: run_id.clone(),
        accepted: journal.accepted,
        failures: journal.failures,
        interrupted: cancelled.load(Ordering::Relaxed),
        elapsed_seconds: elapsed,
        bytes_per_second: journal.bytes as f64 / elapsed.max(f64::EPSILON),
        summary: report::summary(&repo, &records)?,
    };
    let mut file = tempfile::NamedTempFile::new_in(repo.root().join("reports"))?;
    serde_json::to_writer_pretty(&mut file, &report)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(repo.root().join("reports").join(format!("{run_id}.json")))
        .context("cannot publish report")?;
    Ok(report)
}
