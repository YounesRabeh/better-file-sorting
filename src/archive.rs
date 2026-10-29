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
