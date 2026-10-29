use crate::{config::SecurityMode, security, storage::valid_hash};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    pub occurrence_id: String,
    pub sha256: String,
    pub size: u64,
    pub file_type: String,
    pub original_name: String,
    pub original_entry_name: String,
    pub original_stem: String,
    pub original_extension: String,
    pub source_archive: String,
    pub source_path: String,
    pub entry_index: usize,
    pub encrypted: bool,
    pub ingest_date: String,
    pub run_id: String,
}
/// Hash bytes as lowercase hexadecimal SHA-256.
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Occurrence {
    /// Validate identity and file-type fields loaded from a manifest.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            valid_hash(&self.sha256) && valid_hash(&self.occurrence_id),
            "invalid manifest identity"
        );
        ensure!(
            [
                "pe", "elf", "macho", "pdf", "zip", "text", "other", "unknown"
            ]
            .contains(&self.file_type.as_str()),
            "invalid manifest file type"
        );
        Ok(())
    }
}
/// Append one serialized JSON record and synchronize the file.
pub fn append(writer: &mut fs::File, record: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.sync_data()?;
    Ok(())
}
/// Load, validate, deduplicate, and deterministically order manifest records.
pub fn load(root: &Path) -> Result<Vec<Occurrence>> {
    crate::discovery::check_path(&root.join("manifests"))?;
    let mut paths: Vec<_> = fs::read_dir(root.join("manifests"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    let mut records = BTreeMap::new();
    for path in paths {
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        security::validate_file(&path, SecurityMode::Standard)?;
        let mut reader = BufReader::new(fs::File::open(path)?);
        loop {
            let mut line = Vec::new();
            // Bound each untrusted metadata record independently.
            let count =
                std::io::Read::take(&mut reader, 1024 * 1024 + 1).read_until(b'\n', &mut line)?;
            if count == 0 {
                break;
            }
            ensure!(count <= 1024 * 1024, "manifest record exceeds 1 MiB");
            if !line.ends_with(b"\n") {
                tracing::warn!("ignoring interrupted final manifest record");
                break;
            }
            let record: Occurrence = serde_json::from_slice(&line)?;
            record.validate()?;
            if let Some(previous) = records.get(&record.occurrence_id) {
                let previous: &Occurrence = previous;
                ensure!(
                    previous.sha256 == record.sha256 && previous.size == record.size,
                    "conflicting occurrence metadata"
                );
            } else {
                records.insert(record.occurrence_id.clone(), record);
            }
        }
    }
    let mut result: Vec<Occurrence> = records.into_values().collect();
    result.sort_by(|a, b| {
        (&a.source_path, a.entry_index, &a.occurrence_id).cmp(&(
            &b.source_path,
            b.entry_index,
            &b.occurrence_id,
        ))
    });
    Ok(result)
}
