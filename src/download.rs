// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Transfers policy-selected resources into private, sealed staging with completion receipts.
//! Workers share request controls and byte budgets; files remain inaccessible by path
//! while their creation handles receive data. Only synchronized completed payloads are
//! published. Reruns validate sealed cache records, and failures never become receipts.

/// Maximum body read chunk, also bounding the speed limiter burst.
const TRANSFER_BUFFER_BYTES: usize = 16 * 1024;
/// Maximum sanitized label length, leaving room for the source hash and extension.
const MAX_FILENAME_BYTES: usize = 240;

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
    use unicode_normalization::UnicodeNormalization;
    let mut paths = std::collections::BTreeSet::new();
    for candidate in &candidates {
        ensure!(
            crate::storage::valid_hash(&candidate.source_id)
                && candidate.source_id == crate::metadata::digest(candidate.url.as_bytes()),
            "invalid source identity"
        );
        crate::http_source::safe_url(&candidate.url)?;
        let path = payload_path(config, candidate);
        ensure!(
            paths.insert(
                path.to_string_lossy()
                    .nfc()
                    .collect::<String>()
                    .to_lowercase()
            ),
            "download destination collision for {}; choose filename = '{{name}}-{{source_id}}' or '{{source_id}}'",
            crate::presentation::safe_text(&candidate.name)
        );
    }
    Ok(DownloadReport {
        filtered: inventory.candidates.len() - candidates.len(),
        candidates,
        ..Default::default()
    })
}
pub(crate) fn safe_component(text: &str, limit: usize) -> String {
    let mut value = String::new();
    for c in text.chars() {
        let c = if c.is_alphanumeric()
            || (c.is_ascii_graphic()
                && !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
            || c == ' '
        {
            c
        } else {
            '_'
        };
        if value.len() + c.len_utf8() > limit {
            break;
        }
        value.push(c);
    }
    let mut value = value.trim_matches(['.', ' ']).to_string();
    if value.is_empty() {
        value.push_str("download");
    }
    let stem = value.split('.').next().unwrap().to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        || ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"].contains(&stem.as_str())
    {
        value.insert(0, '_');
    }
    value
}
pub fn payload_path(config: &DownloadConfig, candidate: &Candidate) -> PathBuf {
    let (raw_stem, raw_extension) = candidate
        .name
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
        .map_or((candidate.name.as_str(), None), |(stem, ext)| {
            (stem, Some(ext))
        });
    let extension = raw_extension
        .map(|ext| format!(".{}", safe_component(ext, 80)))
        .unwrap_or_default();
    let stem = safe_component(raw_stem, MAX_FILENAME_BYTES - extension.len() - 1);
    let original = format!("{stem}{extension}");
    let id = safe_component(&candidate.source_id, 64);
    let filename = match config.filename.as_str() {
        "{source_id}" => format!("{id}{extension}"),
        "{name}-{source_id}" => {
            let stem = safe_component(
                &stem,
                MAX_FILENAME_BYTES
                    .saturating_sub(id.len() + extension.len() + 1)
                    .max(8),
            );
            format!("{stem}-{id}{extension}")
        }
        _ => original,
    };
    PathBuf::from("payloads")
        .join(&config.directory)
        .join(filename)
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
    completed: usize,
    failed: usize,
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
        completed: 0,
        failed: 0,
    });
    ensure!(
        budget.lock().unwrap().staged <= staging_limit,
        "staging size limit reached"
    );
    let mut work = vec![];
    for candidate in &report.candidates {
        let receipt = receipt_path(config, &candidate.source_id);
        if receipt.try_exists()? {
            let saved = load_receipt(&receipt)?;
            ensure!(
                saved.version == 2
                    && saved.candidate.source_id == candidate.source_id
                    && saved.candidate.url == candidate.url
                    && crate::storage::valid_hash(&saved.sha256)
                    && saved.path == payload_path(config, &saved.candidate),
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
            let target = config.staging_dir.join(payload_path(config, candidate));
            crate::discovery::check_path(&target)?;
            ensure!(
                !target.try_exists()?,
                "download destination collision: {}; existing content retained",
                target.display()
            );
            work.push(candidate.clone());
        } else {
            report.not_attempted += 1;
        }
    }
    let progress = crate::presentation::Progress::new(None, "Collecting");
    let selected = report.candidates.len();
    budget.lock().unwrap().completed = report.cached;
    progress.0.set_message(format!(
        "{}/{} files, 0 failed, 0 bytes",
        report.cached, selected
    ));
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
                        builder.permissions(std::fs::Permissions::from_mode(0o000));
                    }
                    let mut file = builder.tempfile_in(&payloads)?;
                    security::seal(file.as_file(), SecurityMode::Sealed)?;
                    {
                        let b = budget.lock().unwrap();
                        ensure!(
                            b.transferred < total_limit && b.staged < staging_limit,
                            "download byte limit reached"
                        );
                    }
                    let (mut response, _) = http.get(&candidate.url, cancelled)?;
                    let expected = response
                        .headers()
                        .get("content-length")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok());
                    let mut reader = response.body_mut().as_reader();
                    let mut buffer = [0u8; TRANSFER_BUFFER_BYTES];
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
                        if allowance == 0 && size == file_limit && b.transferred < total_limit {
                            // Probe EOF without writing beyond the file cap; charge any observed byte.
                            let n = reader.read(&mut buffer[..1])?;
                            b.transferred += n as u64;
                            ensure!(n == 0, "file size limit reached");
                            break;
                        }
                        ensure!(
                            allowance > 0,
                            "download byte limit reached; EOF not verified"
                        );
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
                        let current = if config.concurrency == 1 {
                            match expected.filter(|length| *length > 0) {
                                Some(length) => format!(
                                    "{}: {:.0}% ({}/{length} bytes); ",
                                    crate::presentation::safe_text(&candidate.name),
                                    (size + n as u64) as f64 * 100.0 / length as f64,
                                    size + n as u64
                                ),
                                None => format!(
                                    "{}: {} bytes; ",
                                    crate::presentation::safe_text(&candidate.name),
                                    size + n as u64
                                ),
                            }
                        } else {
                            String::new()
                        };
                        progress.0.set_message(format!(
                            "{current}{}/{} files, {} failed, {} bytes",
                            b.completed, selected, b.failed, b.transferred
                        ));
                        drop(b);
                        size += n as u64;
                        hash.update(&buffer[..n]);
                        file.write_all(&buffer[..n])?;
                    }
                    ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
                    ensure!(started.elapsed() < timeout, "transfer timeout");
                    ensure!(
                        expected.is_none_or(|length| length == size),
                        "download Content-Length mismatch"
                    );
                    file.as_file().sync_all()?;
                    let path = file.into_temp_path();
                    path.persist_noclobber(&target)?;
                    File::open(target.parent().unwrap())?.sync_all()?;
                    let receipt = DownloadReceipt {
                        version: 2,
                        candidate: candidate.clone(),
                        path: relative,
                        size,
                        sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
                    };
                    save_json(&receipt_path(config, &candidate.source_id), &receipt)?;
                    Ok(())
                })();
                {
                    let mut b = budget.lock().unwrap();
                    if result.is_ok() {
                        b.completed += 1;
                    } else {
                        b.failed += 1;
                    }
                    progress.0.set_message(format!(
                        "{}/{} files, {} failed, {} bytes",
                        b.completed, selected, b.failed, b.transferred
                    ));
                }
                results.lock().unwrap().push(result.map_err(|e| {
                    format!("{}: {e:#}", crate::presentation::safe_text(&candidate.name))
                }));
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
    report.bytes = budget.into_inner().unwrap().transferred;
    Ok(report)
}
