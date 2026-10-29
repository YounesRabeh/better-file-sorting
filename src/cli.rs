use crate::config::SecurityMode;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Organize ZIP samples without executing their contents"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Extract, classify and securely store archive contents
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
