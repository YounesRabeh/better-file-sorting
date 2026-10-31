// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Declares the public command-line interface and clap argument metadata.
//! Commands separate discovery, download, ingest, inspection, statistics and reorganization.
//! Parsing does not perform filesystem or network work; main dispatches commands and
//! configuration merging resolves optional overrides before the pipeline runs.

use crate::config::SecurityMode;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Sort files or ZIP samples by metadata without executing their contents"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    #[command(flatten)]
    pub presentation: Presentation,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Discover if needed, select and download into sealed staging
    Collect(Collect),
    /// Organize ordinary files; --extract explicitly opens ZIP contents
    Organize(Ingest),
    /// Validate configuration without processing inputs
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Discover direct ZIP links from an HTTP page
    Discover(Discover),
    /// Download policy-selected ZIPs into sealed staging
    Download(Acquire),
    /// Classify and sort archive contents or ordinary files; optionally ZIP the output
    Ingest(Ingest),
    /// Inspect archive metadata without extracting payloads
    Inspect(Inspect),
    /// Summarize stored metadata without reading payloads
    Stats {
        /// Repository to summarize; defaults to ./organized
        #[arg(default_value = crate::config::DEFAULT_REPOSITORY)]
        repository: PathBuf,
    },
    /// Build a new view from metadata without unlocking payloads
    Reorganize {
        #[arg(default_value = crate::config::DEFAULT_REPOSITORY)]
        repository: PathBuf,
        #[arg(long)]
        rules: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
}
#[derive(Debug, Args)]
pub struct Ingest {
    /// Sort ordinary files (default), or explicitly select archives
    #[arg(long, value_enum)]
    pub input_mode: Option<crate::config::InputMode>,
    /// Extract ZIP contents using bounded archive processing
    #[arg(long, conflicts_with = "input_mode")]
    pub extract: bool,
    /// Publish only a compressed ZIP at --output, leaving sources untouched
    #[arg(long)]
    pub zip: bool,
    /// ZIP only this sorted relative directory; repeat to select several
    #[arg(long = "zip-dir", value_name = "DIRECTORY")]
    pub zip_dirs: Vec<String>,
    /// Create one ZIP per selected directory, or per top-level directory
    #[arg(long)]
    pub zip_each_dir: bool,
    /// Protected file containing exactly one password for AES-256 ZIP output
    #[arg(long)]
    pub output_password_file: Option<PathBuf>,
    /// Existing directory for private ZIP and preview temporary storage
    #[arg(long)]
    pub staging_dir: Option<PathBuf>,
    pub inputs: Vec<PathBuf>,
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub rules: Option<PathBuf>,
    #[arg(short = 'j', long)]
    pub workers: Option<usize>,
    #[arg(long)]
    pub password_file: Option<PathBuf>,
    #[arg(long, value_enum)]
    pub security_mode: Option<SecurityMode>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub recursive: Option<bool>,
    /// Hash and classify inputs to preview the final layout without publishing
    #[arg(long)]
    pub dry_run: bool,
}
#[derive(Debug, Args)]
pub struct Inspect {
    pub inputs: Vec<PathBuf>,
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub recursive: Option<bool>,
}

#[derive(Debug, Args)]
pub struct Acquire {
    /// Optional runtime TOML; defaults read ./collection/inventory.jsonl
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
}

/// Discovery accepts a direct page URL, optional runtime overrides, or both.
#[derive(Debug, Args)]
pub struct Discover {
    /// HTTP(S) page URL; overrides source.page when a config is supplied
    pub url: Option<String>,
    /// Optional runtime TOML; omitted settings use built-in acquisition defaults
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,
    /// Discovery requires network access; use download --dry-run for offline selection
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct Collect {
    /// HTTP(S) page; omitted when source.page is configured
    pub url: Option<String>,
    #[arg(short = 'c', long)]
    pub config: Option<PathBuf>,
    /// Collection root containing inventory.jsonl and downloads/
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    #[arg(short = 'j', long)]
    pub workers: Option<usize>,
    /// Rediscover instead of reusing the saved inventory
    #[arg(long)]
    pub refresh: bool,
    /// Select without payload requests; missing inventory requires an HTML metadata request
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Check TOML fields and security/limit settings
    Validate {
        #[arg(short = 'c', long)]
        config: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Args)]
pub struct Presentation {
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorMode,
    /// Disable color (takes precedence over --color)
    #[arg(long, global = true)]
    pub no_color: bool,
    #[arg(long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
    #[arg(long, global = true)]
    pub verbose: bool,
    /// Machine-readable stdout; disables ANSI and progress on both streams
    #[arg(long, global = true)]
    pub json: bool,
}
