// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Inspects ZIP metadata and exports rule-planned layouts as ZIP archives.
//! Input opening enforces configured archive limits; output publication uses private
//! temporary storage and refuses replacement of existing destinations. Export selection
//! is shared with preview so users see the same directory choices before publication.

use crate::{config::Limits, discovery::check_path};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs::File,
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive};
#[derive(Debug, Serialize)]
pub struct Entry {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
    pub encrypted: bool,
    pub compression: String,
    pub supported_compression: bool,
    pub directory: bool,
    pub unix_mode: Option<u32>,
}
#[derive(Debug, Serialize)]
pub struct Inspection {
    pub archive: String,
    pub entries: Vec<Entry>,
}

#[derive(Serialize)]
pub struct ZipExport {
    pub output: PathBuf,
    pub entries: Vec<crate::rules::Planned>,
}

/// Resolve exact sorted-directory selections once for previews and publication.
pub fn plan_exports(
    plan: &[crate::rules::Planned],
    config: &crate::config::Config,
) -> Result<Vec<ZipExport>> {
    config.validate()?;
    let directories: BTreeSet<String> = if config.zip_dirs.is_empty() && config.zip_each_dir {
        plan.iter()
            .filter_map(|p| p.path.split_once('/').map(|(d, _)| d.to_string()))
            .collect()
    } else {
        config.zip_dirs.iter().cloned().collect()
    };
    let matches = |entry: &crate::rules::Planned, directory: &str| {
        entry.path.starts_with(&format!("{directory}/"))
    };
    for directory in &directories {
        ensure!(
            plan.iter().any(|p| matches(p, directory)),
            "sorted directory not found: {directory}"
        );
    }
    if !config.zip_each_dir {
        let entries = plan
            .iter()
            .filter(|p| directories.is_empty() || directories.iter().any(|d| matches(p, d)))
            .cloned()
            .collect();
        return Ok(vec![ZipExport {
            output: config.output.clone(),
            entries,
        }]);
    }
    ensure!(!directories.is_empty(), "no sorted directories to ZIP");
    let exports: Vec<_> = directories
        .iter()
        .map(|directory| ZipExport {
            output: config.output.join(format!("{directory}.zip")),
            entries: plan
                .iter()
                .filter(|p| matches(p, directory))
                .cloned()
                .collect(),
        })
        .collect();
    for (index, a) in exports.iter().enumerate() {
        for b in &exports[index + 1..] {
            ensure!(
                !a.output.starts_with(&b.output) && !b.output.starts_with(&a.output),
                "ZIP output file conflicts with an output directory"
            );
        }
    }
    Ok(exports)
}

/// Build every ZIP before publishing a new output directory as one generation.
pub fn publish_exports(
    repo: &crate::storage::Repository,
    exports: &[ZipExport],
    config: &crate::config::Config,
    password: Option<&str>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    if !config.zip_each_dir {
        let export = exports
            .first()
            .ok_or_else(|| anyhow::anyhow!("no ZIP export planned"))?;
        return publish(repo, &export.entries, &export.output, password, cancelled);
    }
    let parent = config
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = crate::security::private_tempdir(parent, ".bfs-exports-")?;
    for export in exports {
        let relative = export.output.strip_prefix(&config.output)?;
        let mut directory = staging.path().to_path_buf();
        for part in relative.parent().unwrap_or(Path::new("")).components() {
            ensure!(
                matches!(part, std::path::Component::Normal(_)),
                "unsafe ZIP output path"
            );
            directory.push(part);
            crate::security::private_dir(&directory)?;
        }
        publish(
            repo,
            &export.entries,
            &staging.path().join(relative),
            password,
            cancelled,
        )?;
    }
    ensure!(
        !cancelled.load(std::sync::atomic::Ordering::Relaxed),
        "interrupted"
    );
    check_path(&config.output)?;
    ensure!(!config.output.try_exists()?, "ZIP output already exists");
    crate::security::publish_directory(staging.path(), &config.output)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
/// Open a ZIP archive after enforcing input-size and entry-count limits.
pub fn open(path: &Path, limits: &Limits) -> Result<ZipArchive<File>> {
    check_path(path)?;
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= limits.max_input_bytes,
        "archive input size limit exceeded"
    );
    let archive = ZipArchive::new(file)?;
    ensure!(
        archive.len() <= limits.max_entries,
        "archive entry count limit exceeded"
    );
    Ok(archive)
}
/// Read archive entry metadata without extracting payloads.
pub fn inspect(path: &Path, limits: &Limits) -> Result<Inspection> {
    let archive = open(path, limits)?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let e = archive.by_index_data(index)?;
        entries.push(Entry {
            index,
            name: e.name()?.into_owned(),
            size: e.size(),
            compressed_size: e.compressed_size(),
            encrypted: e.encrypted(),
            compression: format!("{:?}", e.compression()),
            supported_compression: matches!(
                e.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            ),
            directory: e.is_dir(),
            unix_mode: e.unix_mode(),
        });
    }
    Ok(Inspection {
        archive: path.to_string_lossy().into_owned(),
        entries,
    })
}

/// Write the logical policy layout, then atomically publish a private ZIP.
pub fn publish(
    repo: &crate::storage::Repository,
    plan: &[crate::rules::Planned],
    output: &Path,
    password: Option<&str>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    use std::{
        io::{Read, Write},
        sync::atomic::Ordering,
    };
    check_path(output)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    let mut zip = zip::ZipWriter::new(file.as_file_mut());
    let mut options = zip::write::SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    if let Some(password) = password {
        options = options.with_aes_encryption(zip::AesMode::Aes256, password);
    }
    let mut buffer = [0u8; 65536];
    for entry in plan {
        ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
        let source = repo.object(&entry.sha256)?;
        crate::security::validate_file(&source, crate::config::SecurityMode::Standard)?;
        let mut source = File::open(source)?;
        zip.start_file(&entry.path, options)?;
        loop {
            ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            zip.write_all(&buffer[..count])?;
        }
    }
    zip.start_file("layout.jsonl", options)?;
    for entry in plan {
        serde_json::to_writer(&mut zip, entry)?;
        zip.write_all(b"\n")?;
    }
    zip.finish()?;
    file.as_file().sync_all()?;
    ensure!(!cancelled.load(Ordering::Relaxed), "interrupted");
    file.persist_noclobber(output)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
