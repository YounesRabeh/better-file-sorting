use crate::{config::Limits, discovery::check_path};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{fs::File, path::Path};
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
