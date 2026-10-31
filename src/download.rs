//! Bounded transfers into sealed, receipt-owned staging.
use crate::{
    acquisition::*,
    config::SecurityMode,
    http_source::{HttpSource, wait_until},
    security,
};
use anyhow::{Result, ensure};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub fn preview(config: &DownloadConfig, inventory: &Inventory) -> Result<DownloadReport> {
    let candidates = select_candidates(inventory, config)?;
    Ok(DownloadReport {
        filtered: inventory.candidates.len() - candidates.len(),
        candidates,
        ..Default::default()
    })
}
fn safe_component(text: &str) -> String {
    let value: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if value.is_empty() {
        "download".into()
    } else {
        format!("item_{value}")
    }
}
pub fn payload_path(config: &DownloadConfig, candidate: &Candidate) -> PathBuf {
    let directory = match config.directory.as_str() {
        "{title}" => safe_component(&candidate.title),
        "{name}" => safe_component(&candidate.name),
        _ => config.directory.clone(),
    };
    PathBuf::from("payloads").join(directory).join(format!(
        "{}-{}.zip",
        safe_component(candidate.name.trim_end_matches(".zip")),
        candidate.source_id
    ))
}
fn receipt_path(config: &DownloadConfig, id: &str) -> PathBuf {
    config
        .staging_dir
        .join("control")
        .join(format!("{id}.json"))
}
fn staged_bytes(root: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        ensure!(!entry.file_type().is_symlink(), "staging symlink refused");
        if entry.file_type().is_file() {
            total = total
                .checked_add(entry.metadata()?.len())
                .ok_or_else(|| anyhow::anyhow!("staging size overflow"))?;
        }
    }
    Ok(total)
}
struct Budget {
    transferred: u64,
    staged: u64,
    pace: Instant,
}
pub fn run(
    config: &DownloadConfig,
    source: &SourceConfig,
    inventory: &Inventory,
    cancelled: &AtomicBool,
) -> Result<DownloadReport> {
    let mut report = preview(config, inventory)?;
    private_tree(&config.staging_dir)?;
    security::private_dir(&config.staging_dir)?;
    let control = config.staging_dir.join("control");
    private_tree(&control)?;
    security::private_dir(&control)?;
    let payloads = config.staging_dir.join("payloads");
    private_tree(&payloads)?;
    let lock_path = control.join(".lock");
    crate::discovery::check_path(&lock_path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(lock_path)?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("download staging is in use"))?;
    let http = HttpSource::new(source, config)?;
    let total_limit = parse_bytes(&config.max_total_size)?;
    let staging_limit = parse_bytes(&config.max_staging_size)?;
    let file_limit = parse_bytes(&config.max_file_size)?;
    let speed = config.speed()?;
    let budget = Mutex::new(Budget {
        transferred: 0,
        staged: staged_bytes(&payloads)?,
        pace: Instant::now(),
    });
    ensure!(
        budget.lock().unwrap().staged <= staging_limit,
        "staging size limit reached"
    );
    let mut work = vec![];
    for candidate in &report.candidates {
        let receipt = receipt_path(config, &candidate.source_id);
        if receipt.try_exists()? {
            let saved: DownloadReceipt = load_json(&receipt)?;
            ensure!(
                saved.version == 1
                    && saved.candidate.source_id == candidate.source_id
                    && saved.path == payload_path(config, candidate),
                "invalid download receipt"
            );
            let path = config.staging_dir.join(&saved.path);
            security::validate_file(&path, SecurityMode::Sealed)?;
            ensure!(
                std::fs::metadata(path)?.len() == saved.size,
                "cached file changed"
            );
            report.cached += 1;
        } else if work.len() < config.max_files {
            work.push(candidate.clone());
        } else {
            report.not_attempted += 1;
        }
    }
    let results = Mutex::new(vec![]);
    rayon::ThreadPoolBuilder::new()
        .num_threads(config.concurrency)
        .build()?
        .install(|| {
            work.par_iter().for_each(|candidate| {
                let result = (|| -> Result<()> {
                    ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
                    let started = Instant::now();
                    let timeout = Duration::from_secs(config.timeout_seconds);
                    let relative = payload_path(config, candidate);
                    let target = config.staging_dir.join(&relative);
                    private_tree(target.parent().unwrap())?;
                    crate::discovery::check_path(&target)?;
                    ensure!(!target.try_exists()?, "unreceipted target exists");
                    let mut builder = tempfile::Builder::new();
                    builder.prefix(".partial-");
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        builder.permissions(std::fs::Permissions::from_mode(0));
                    }
                    let mut file = builder.tempfile_in(&payloads)?;
                    security::seal(file.as_file(), SecurityMode::Sealed)?;
                    let (mut response, _) = http.get(&candidate.url, cancelled)?;
                    let expected = response
                        .headers()
                        .get("content-length")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok());
                    let mut reader = response.body_mut().as_reader();
                    let mut buffer = [0u8; 16384];
                    let mut size = 0u64;
                    let mut hash = Sha256::new();
                    loop {
                        if expected == Some(size) {
                            break;
                        }
                        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
                        ensure!(started.elapsed() < timeout, "transfer timeout");
                        // Serialize read reservations so concurrent transfers cannot overshoot aggregate budgets.
                        let mut b = budget.lock().unwrap();
                        let allowance = (total_limit - b.transferred)
                            .min(staging_limit - b.staged)
                            .min(file_limit - size)
                            .min(buffer.len() as u64)
                            as usize;
                        ensure!(allowance > 0, "download byte limit reached");
                        let n = reader.read(&mut buffer[..allowance])?;
                        if n == 0 {
                            break;
                        }
                        b.transferred += n as u64;
                        b.staged += n as u64;
                        if let Some(rate) = speed {
                            b.pace = b.pace.max(Instant::now())
                                + Duration::from_secs_f64(n as f64 / rate as f64);
                            wait_until(b.pace.min(started + timeout), cancelled)?;
                        }
                        drop(b);
                        size += n as u64;
                        hash.update(&buffer[..n]);
                        file.write_all(&buffer[..n])?;
                    }
                    file.as_file().sync_all()?;
                    let path = file.into_temp_path();
                    path.persist_noclobber(&target)?;
                    File::open(target.parent().unwrap())?.sync_all()?;
                    let receipt = DownloadReceipt {
                        version: 1,
                        candidate: candidate.clone(),
                        path: relative,
                        size,
                        sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
                    };
                    save_json(&receipt_path(config, &candidate.source_id), &receipt)?;
                    Ok(())
                })();
                results
                    .lock()
                    .unwrap()
                    .push(result.map_err(|e| format!("{}: {e}", candidate.source_id)));
            })
        });
    for result in results.into_inner().unwrap() {
        match result {
            Ok(()) => report.downloaded += 1,
            Err(e) => {
                report.failed += 1;
                report.errors.push(e);
            }
        }
    }
    Ok(report)
}
