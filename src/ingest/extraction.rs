// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Streams ordinary files or individual ZIP entries into sealed temporary storage.
//! Each copy computes SHA-256 and a bounded classification prefix while enforcing
//! configured size and expansion limits. Archive names, entry kinds, integrity and
//! cancellation are checked before extracted content can become a stored object.

use crate::app::config::Limits;
use anyhow::{Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Extracted {
    pub file: tempfile::NamedTempFile,
    pub sha256: String,
    pub size: u64,
    pub prefix: Vec<u8>,
}

/// Copy and hash one ordinary file into private staging with bounded reads.
pub fn extract_file(
    path: &Path,
    staging: &Path,
    limits: &Limits,
    cancelled: &AtomicBool,
) -> Result<Extracted> {
    crate::ingest::discovery::check_path(path)?;
    ensure!(
        std::fs::symlink_metadata(path)?.is_file(),
        "not a regular file"
    );
    let mut reader = File::open(path)?;
    let limit = [
        limits.max_entry_bytes,
        limits.max_input_bytes,
        limits.max_archive_bytes,
    ]
    .into_iter()
    .flatten()
    .min();
    let input_size = reader.metadata()?.len();
    ensure!(
        limit.is_none_or(|limit| input_size <= limit),
        "file size limit exceeded"
    );
    let mut file = tempfile::NamedTempFile::new_in(staging)?;
    crate::repository::security::seal(file.as_file(), crate::app::config::SecurityMode::Sealed)?;
    let mut hash = Sha256::new();
    let mut prefix = Vec::with_capacity(8192);
    let mut size = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
        ensure!(
            limit.is_none_or(|limit| size <= limit),
            "file size limit exceeded"
        );
        hash.update(&buffer[..count]);
        prefix.extend_from_slice(&buffer[..count.min(8192 - prefix.len())]);
        file.write_all(&buffer[..count])?;
    }
    file.as_file().sync_all()?;
    Ok(Extracted {
        file,
        sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
        size,
        prefix,
    })
}
/// Validate an archive entry name before using it as metadata.
pub fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && !name.starts_with(['/', '\\']),
        "absolute or empty archive name"
    );
    ensure!(
        !name.contains(':') && !name.chars().any(char::is_control),
        "unsafe archive name"
    );
    ensure!(
        !name.split(['/', '\\']).any(|c| c == ".."),
        "archive path traversal"
    );
    Ok(())
}
/// Stream one archive entry into private staging while hashing and validating limits.
pub fn extract(
    archive: &mut zip::ZipArchive<File>,
    index: usize,
    passwords: &[String],
    staging: &Path,
    limits: &Limits,
    total: &mut u64,
    cancelled: &AtomicBool,
) -> Result<Extracted> {
    let entry = archive.by_index_data(index)?;
    validate_name(&entry.name()?)?;
    ensure!(!entry.is_dir(), "directory is not a sample");
    let kind = entry.unix_mode().unwrap_or(0) & 0o170000;
    ensure!(
        kind == 0 || kind == 0o100000,
        "special archive entry refused"
    );
    let expected_size = entry.size();
    let compressed = entry.compressed_size().max(1);
    ensure!(
        limits
            .max_entry_bytes
            .is_none_or(|limit| expected_size <= limit),
        "entry size limit exceeded"
    );
    ensure!(
        limits
            .max_archive_bytes
            .is_none_or(|limit| expected_size <= limit.saturating_sub(*total)),
        "archive size limit exceeded"
    );
    let ratio_limit = limits
        .max_expansion_ratio
        .and_then(|ratio| compressed.checked_mul(ratio));
    ensure!(
        ratio_limit.is_none_or(|limit| expected_size <= limit),
        "expansion ratio limit exceeded"
    );
    let encrypted = entry.encrypted();
    let candidates: Vec<Option<&str>> = if encrypted {
        passwords.iter().map(|s| Some(s.as_str())).collect()
    } else {
        vec![None]
    };
    for password in candidates {
        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
        let opened = match password {
            Some(p) => archive.by_index_decrypt(index, p.as_bytes()),
            None => archive.by_index(index),
        };
        let mut reader = match opened {
            Ok(reader) => reader,
            Err(_) if encrypted => continue,
            Err(e) => return Err(e.into()),
        };
        let builder = tempfile::Builder::new();
        #[cfg(unix)]
        let mut builder = builder;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The creation handle stays writable; a crash never leaves a reopenable payload.
            builder.permissions(std::fs::Permissions::from_mode(0o000));
        }
        let mut file = builder.tempfile_in(staging)?;
        let mut hash = Sha256::new();
        let mut prefix = Vec::with_capacity(8192);
        let mut size = 0u64;
        let mut buffer = [0u8; 65536];
        let mut invalid = false;
        loop {
            ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
            let count = match reader.read(&mut buffer) {
                Ok(n) => n,
                Err(_) => {
                    invalid = true;
                    break;
                }
            };
            if count == 0 {
                break;
            }
            size = size
                .checked_add(count as u64)
                .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
            *total = total
                .checked_add(count as u64)
                .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
            ensure!(
                limits.max_entry_bytes.is_none_or(|limit| size <= limit)
                    && ratio_limit.is_none_or(|limit| size <= limit)
                    && limits.max_archive_bytes.is_none_or(|limit| *total <= limit),
                "actual decompression limit exceeded"
            );
            hash.update(&buffer[..count]);
            let prefix_count = count.min(8192 - prefix.len());
            prefix.extend_from_slice(&buffer[..prefix_count]);
            file.write_all(&buffer[..count])?;
        }
        if invalid || size != expected_size {
            continue;
        }
        file.as_file().sync_all()?;
        return Ok(Extracted {
            file,
            sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
            size,
            prefix,
        });
    }
    bail!("entry decryption or integrity verification failed")
}
