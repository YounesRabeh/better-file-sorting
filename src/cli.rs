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
    /// Classify and sort archive contents or ordinary files; optionally ZIP the output
    Ingest(Ingest),
    /// Inspect archive metadata without extracting payloads
    Inspect(Inspect),
    /// Summarize stored metadata without reading payloads
    Stats { repository: PathBuf },
    /// Build a new view from metadata without unlocking payloads
    Reorganize {
        repository: PathBuf,
        #[arg(long)]
        rules: PathBuf,
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
