use crate::{cli::Ingest, error::Error};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    #[default]
    Archives,
    Files,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SecurityMode {
    Standard,
    #[default]
    Sealed,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Security {
    pub mode: SecurityMode,
    pub restrict_directories: bool,
    pub allow_automatic_unsealing: bool,
}
impl Default for Security {
    /// Return the sealed, private security defaults.
    fn default() -> Self {
        Self {
            mode: SecurityMode::Sealed,
            restrict_directories: true,
            allow_automatic_unsealing: false,
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_entry_bytes: u64,
    pub max_archive_bytes: u64,
    pub max_input_bytes: u64,
    pub max_entries: usize,
    pub max_expansion_ratio: u64,
}
impl Default for Limits {
    /// Return conservative extraction and expansion limits.
    fn default() -> Self {
        Self {
            max_entry_bytes: 256 * 1024 * 1024,
            max_archive_bytes: 1024 * 1024 * 1024,
            max_input_bytes: 1024 * 1024 * 1024,
            max_entries: 10_000,
            max_expansion_ratio: 1000,
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub input_mode: InputMode,
    pub zip: bool,
    pub output_password_file: Option<PathBuf>,
    pub inputs: Vec<PathBuf>,
    pub output: PathBuf,
    pub recursive: bool,
    pub workers: usize,
    pub password_file: Option<PathBuf>,
    pub rules: Option<PathBuf>,
    pub security: Security,
    pub limits: Limits,
}
impl Default for Config {
    /// Return configuration defaults with bounded worker parallelism.
    fn default() -> Self {
        Self {
            input_mode: InputMode::Archives,
            zip: false,
            output_password_file: None,
            inputs: vec![],
            output: "repository".into(),
            recursive: true,
            workers: std::thread::available_parallelism().map_or(1, |n| n.get().min(8)),
            password_file: None,
            rules: None,
            security: Security::default(),
            limits: Limits::default(),
        }
    }
}
impl Config {
    /// Load TOML configuration from an optional path.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(path) => {
                toml::from_str(&fs::read_to_string(path).context("cannot read configuration")?)
                    .map_err(|_| {
                        Error::Config("invalid TOML, unknown field, or unsupported value".into())
                            .into()
                    })
            }
            None => Ok(Self::default()),
        }
    }
    /// Merge CLI ingest overrides into file or default configuration.
    pub fn for_ingest(args: &Ingest) -> Result<Self> {
        let mut c = Self::load(args.config.as_deref())?;
        if let Some(mode) = args.input_mode {
            c.input_mode = mode;
        }
        if args.zip {
            c.zip = true;
        }
        if let Some(path) = &args.output_password_file {
            c.output_password_file = Some(path.clone());
        }
        if !args.inputs.is_empty() {
            c.inputs.clone_from(&args.inputs);
        }
        if let Some(v) = &args.output {
            c.output.clone_from(v);
        }
        if let Some(v) = &args.rules {
            c.rules = Some(v.clone());
        }
        if let Some(v) = args.workers {
            c.workers = v;
        }
        if let Some(v) = &args.password_file {
            c.password_file = Some(v.clone());
        }
        if let Some(v) = args.security_mode {
            c.security.mode = v;
        }
        if let Some(v) = args.recursive {
            c.recursive = v;
        }
        c.validate()?;
        Ok(c)
    }
    /// Validate worker, security, output, and extraction-limit settings.
    pub fn validate(&self) -> Result<()> {
        let error = |s: &str| Error::Config(s.into());
        if self.output_password_file.is_some() && !self.zip {
            return Err(error("output_password_file requires ZIP output").into());
        }
        if !(1..=256).contains(&self.workers) {
            return Err(error("workers must be between 1 and 256").into());
        }
        if !self.security.restrict_directories || self.security.allow_automatic_unsealing {
            return Err(error(
                "private directories are required; automatic unsealing is forbidden",
            )
            .into());
        }
        if self.limits.max_entry_bytes == 0
            || self.limits.max_archive_bytes == 0
            || self.limits.max_input_bytes == 0
            || self.limits.max_entries == 0
            || self.limits.max_expansion_ratio == 0
        {
            return Err(error("extraction limits must be positive").into());
        }
        if self.limits.max_entry_bytes > self.limits.max_archive_bytes {
            return Err(error("entry limit exceeds archive limit").into());
        }
        if self.output.as_os_str().is_empty() {
            return Err(error("output must not be empty").into());
        }
        Ok(())
    }
}
