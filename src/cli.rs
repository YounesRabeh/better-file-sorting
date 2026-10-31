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
}
#[derive(Debug, Subcommand)]
pub enum Command {
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
    /// Sort ordinary files without opening archives, or extract archives (default)
    #[arg(long, value_enum)]
    pub input_mode: Option<crate::config::InputMode>,
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
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub rules: Option<PathBuf>,
    #[arg(long)]
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
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, action = clap::ArgAction::Set)]
    pub recursive: Option<bool>,
}

#[derive(Debug, Args)]
pub struct Acquire {
    /// Optional runtime TOML; defaults read ./collection/inventory.jsonl
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
}

/// Discovery accepts a direct page URL, optional runtime overrides, or both.
#[derive(Debug, Args)]
pub struct Discover {
    /// HTTP(S) page URL; overrides source.page when a config is supplied
    #[arg(required_unless_present = "config")]
    pub url: Option<String>,
    /// Optional runtime TOML; omitted settings use built-in acquisition defaults
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Discovery requires network access; use download --dry-run for offline selection
    #[arg(long)]
    pub dry_run: bool,
}
