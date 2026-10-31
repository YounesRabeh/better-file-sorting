//! Acquisition configuration and durable inventories, separate from payload storage.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn parse_bytes(value: &str) -> Result<u64> {
    let parts: Vec<_> = value.split_whitespace().collect();
    ensure!(parts.len() == 2, "expected integer and byte unit");
    ensure!(
        parts[0].bytes().all(|b| b.is_ascii_digit()),
        "invalid byte quantity"
    );
    let multiplier = match parts[1] {
        "B" => 1,
        "KB" => 1000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "TB" => 1_000_000_000_000,
        "KiB" => 1024,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => anyhow::bail!("invalid byte unit"),
    };
    let size = parts[0]
        .parse::<u64>()?
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow::anyhow!("byte quantity overflow"))?;
    ensure!(size > 0, "byte quantity must be positive");
    Ok(size)
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceConfig {
    pub page: String,
    pub same_origin: bool,
    pub max_page_size: String,
}
impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            page: String::new(),
            same_origin: true,
            max_page_size: "5 MB".into(),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OrderBy {
    #[default]
    Title,
    Name,
    Date,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    #[default]
    Ascending,
    Descending,
}
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Include,
    Exclude,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub title_contains: Option<String>,
    pub name_contains: Option<String>,
    pub action: Action,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DownloadConfig {
    pub inventory: PathBuf,
    pub staging_dir: PathBuf,
    pub max_files: usize,
    pub max_file_size: String,
    pub max_total_size: String,
    pub max_staging_size: String,
    pub concurrency: usize,
    pub timeout_seconds: u64,
    pub max_speed: Option<String>,
    pub min_request_interval_ms: u64,
    pub max_requests: Option<usize>,
    pub order_by: OrderBy,
    pub order: Order,
    pub directory: String,
    pub policies: Vec<Policy>,
}
impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            inventory: "./collection/inventory.jsonl".into(),
            staging_dir: "./collection/downloads".into(),
            max_files: 100,
            max_file_size: "1 GB".into(),
            max_total_size: "10 GB".into(),
            max_staging_size: "20 GB".into(),
            concurrency: 3,
            timeout_seconds: 300,
            max_speed: None,
            min_request_interval_ms: 0,
            max_requests: None,
            order_by: OrderBy::Title,
            order: Order::Ascending,
            directory: "{title}".into(),
            policies: vec![],
        }
    }
}
impl DownloadConfig {
    pub fn speed(&self) -> Result<Option<u64>> {
        self.max_speed
            .as_deref()
            .map(|s| {
                parse_bytes(
                    s.strip_suffix("/s")
                        .ok_or_else(|| anyhow::anyhow!("speed requires /s"))?,
                )
            })
            .transpose()
    }
    pub fn validate(&self) -> Result<()> {
        for size in [
            &self.max_file_size,
            &self.max_total_size,
            &self.max_staging_size,
        ] {
            parse_bytes(size)?;
        }
        self.speed()?;
        ensure!(
            self.max_files > 0
                && (1..=256).contains(&self.concurrency)
                && self.timeout_seconds > 0
                && self.max_requests != Some(0),
            "invalid download limits"
        );
        ensure!(
            !self.inventory.as_os_str().is_empty() && !self.staging_dir.as_os_str().is_empty(),
            "empty acquisition path"
        );
        for p in &self.policies {
            ensure!(
                p.title_contains.is_some() ^ p.name_contains.is_some(),
                "policy requires exactly one match field"
            );
        }
        if !["{title}", "{name}"].contains(&self.directory.as_str()) {
            ensure!(
                !self.directory.is_empty()
                    && self.directory.split('/').all(|c| !c.is_empty()
                        && c != "."
                        && c != ".."
                        && !c.contains(['\\', ':', '{', '}'])
                        && !c.chars().any(char::is_control)),
                "unsafe download directory"
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub source_id: String,
    pub url: String,
    pub title: String,
    pub name: String,
    pub discovered_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub version: u32,
    pub candidates: Vec<Candidate>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadReceipt {
    pub version: u32,
    pub candidate: Candidate,
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
}
#[derive(Debug, Default, Serialize)]
pub struct DownloadReport {
    pub downloaded: usize,
    pub cached: usize,
    pub filtered: usize,
    pub failed: usize,
    pub not_attempted: usize,
    pub candidates: Vec<Candidate>,
    pub errors: Vec<String>,
}
pub fn select_candidates(inventory: &Inventory, config: &DownloadConfig) -> Result<Vec<Candidate>> {
    config.validate()?;
    let mut rows: Vec<_> = inventory
        .candidates
        .iter()
        .filter(|c| {
            let matches = |p: &Policy| {
                p.title_contains
                    .as_ref()
                    .map(|s| c.title.to_lowercase().contains(&s.to_lowercase()))
                    .unwrap_or_else(|| {
                        c.name
                            .to_lowercase()
                            .contains(&p.name_contains.as_ref().unwrap().to_lowercase())
                    })
            };
            !config
                .policies
                .iter()
                .any(|p| p.action == Action::Exclude && matches(p))
                && (!config.policies.iter().any(|p| p.action == Action::Include)
                    || config
                        .policies
                        .iter()
                        .any(|p| p.action == Action::Include && matches(p)))
        })
        .cloned()
        .collect();
    let key = |c: &Candidate| match config.order_by {
        OrderBy::Title => c.title.to_lowercase(),
        OrderBy::Name => c.name.to_lowercase(),
        OrderBy::Date => c.discovered_at.clone(),
    };
    rows.sort_by(|a, b| {
        let cmp = key(a).cmp(&key(b));
        let cmp = if config.order == Order::Descending {
            cmp.reverse()
        } else {
            cmp
        };
        cmp.then(a.source_id.cmp(&b.source_id))
    });
    Ok(rows)
}
pub fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    crate::discovery::check_path(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    crate::security::validate_file(path, crate::config::SecurityMode::Standard)?;
    let mut bytes = vec![];
    File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "acquisition metadata exceeds 16 MiB"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn load_inventory(path: &Path) -> Result<Inventory> {
    let inventory: Inventory = load_json(path)?;
    ensure!(inventory.version == 1, "unsupported inventory version");
    let mut ids = std::collections::BTreeSet::new();
    for c in &inventory.candidates {
        ensure!(
            c.source_id == crate::metadata::digest(c.url.as_bytes()) && ids.insert(&c.source_id),
            "invalid or duplicate source identity"
        );
    }
    Ok(inventory)
}

/// Create only missing private directories, checking all existing ancestors for links.
pub fn private_tree(path: &Path) -> Result<()> {
    crate::discovery::check_path(path)?;
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        private_tree(parent)?;
    }
    crate::security::private_dir(path)
}

#[derive(Debug)]
pub struct CompletedInput {
    pub path: PathBuf,
    pub receipt: DownloadReceipt,
}
pub fn completed_inputs(config: &DownloadConfig) -> Result<Vec<CompletedInput>> {
    use sha2::{Digest, Sha256};
    let control = config.staging_dir.join("control");
    crate::discovery::check_path(&control)?;
    let mut inputs = vec![];
    for entry in std::fs::read_dir(control)? {
        let entry = entry?;
        if entry.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let receipt: DownloadReceipt = load_json(&entry.path())?;
        ensure!(
            receipt.version == 1
                && crate::storage::valid_hash(&receipt.sha256)
                && receipt.candidate.source_id
                    == crate::metadata::digest(receipt.candidate.url.as_bytes()),
            "invalid download receipt"
        );
        crate::http_source::safe_url(&receipt.candidate.url)?;
        ensure!(
            receipt.path == crate::download::payload_path(config, &receipt.candidate),
            "unsafe receipt path"
        );
        let path = config.staging_dir.join(&receipt.path);
        if crate::security::validate_file(&path, crate::config::SecurityMode::Sealed).is_ok() {
            anyhow::bail!(
                "download is sealed; explicitly unlock selected payloads to 0600 before ingest"
            );
        }
        crate::security::validate_file(&path, crate::config::SecurityMode::Standard)?;
        let mut reader = File::open(&path)?;
        ensure!(
            reader.metadata()?.len() == receipt.size,
            "download size changed"
        );
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        ensure!(actual == receipt.sha256, "download hash changed");
        inputs.push(CompletedInput {
            path: path.canonicalize()?,
            receipt,
        });
    }
    inputs.sort_by(|a, b| {
        a.receipt
            .candidate
            .source_id
            .cmp(&b.receipt.candidate.source_id)
    });
    ensure!(!inputs.is_empty(), "no completed downloads found");
    Ok(inputs)
}
