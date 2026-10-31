// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Runs the BFS command-line application and translates outcomes into process exit codes.
//! The dispatcher connects parsed arguments to configuration, cancellation and library
//! operations. Structured results go to stdout; diagnostics go to stderr so scripts can
//! consume reports without mixing them with progress and error messages.

use anyhow::Result;
use bfs::{
    cli::{Cli, Command},
    config::Config,
};
use clap::Parser;
/// Dispatch one parsed CLI command and return its process exit code.
fn run(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Discover(args) => {
            anyhow::ensure!(
                !args.dry_run,
                "discover performs HTTP requests; use download --dry-run for offline preview"
            );
            let config = Config::load(args.config.as_deref())?;
            let mut source = config.source.unwrap_or_default();
            if let Some(url) = args.url {
                source.page = url;
            }
            let download = config.download.unwrap_or_default();
            let cancelled = cancellation()?;
            let inventory = bfs::http_source::discover(&source, &download, &cancelled)?;
            let parent = download
                .inventory
                .parent()
                .unwrap_or(std::path::Path::new("."));
            bfs::acquisition::private_tree(parent)?;
            bfs::acquisition::save_json(&download.inventory, &inventory)?;
            println!("{}", serde_json::to_string_pretty(&inventory)?);
            Ok(0)
        }
        Command::Download(args) => {
            let config = Config::load(args.config.as_deref())?;
            let download = config.download.unwrap_or_default();
            let inventory = bfs::acquisition::load_inventory(&download.inventory)?;
            let report = if args.dry_run || inventory.candidates.is_empty() {
                bfs::download::preview(&download, &inventory)?
            } else {
                let mut source = config.source.unwrap_or_default();
                if source.page.is_empty() {
                    // Default discovery admits one origin, recover it from the saved links.
                    source.page = inventory.candidates[0].url.clone();
                }
                bfs::download::run(&download, &source, &inventory, cancellation()?.as_ref())?
            };
            let incomplete = report.failed > 0 || report.not_attempted > 0;
            let code = if !incomplete {
                0
            } else if report.downloaded + report.cached > 0 {
                2
            } else {
                1
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(code)
        }
        Command::Ingest(args) => {
            let config = Config::for_ingest(&args)?;
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let signal = cancelled.clone();
            ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
            if args.dry_run {
                let preview = bfs::pipeline::preview(&config, &cancelled)?;
                println!("{}", serde_json::to_string_pretty(&preview)?);
                return Ok(0);
            }
            let report = bfs::pipeline::ingest(&config, &cancelled)?;
            eprintln!(
                "Accepted {} samples; {} failures; {} unique objects ({:?})",
                report.accepted,
                report.failures,
                report.summary.unique_samples,
                report.summary.security_mode
            );
            if let Some(zip) = &report.zip {
                eprintln!(
                    "ZIP: {} payload bytes -> {} output bytes; {} bytes saved; {:.3}s total",
                    zip.payload_bytes, zip.output_bytes, zip.saved_bytes, report.elapsed_seconds
                );
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(if report.failures > 0 || report.interrupted {
                if report.accepted > 0 { 2 } else { 1 }
            } else {
                0
            })
        }
        Command::Inspect(args) => {
            let mut config = Config::load(args.config.as_deref())?;
            if !args.inputs.is_empty() {
                config.inputs = args.inputs;
            }
            if let Some(value) = args.recursive {
                config.recursive = value;
            }
            if config.inputs.is_empty() {
                config
                    .inputs
                    .push(bfs::config::DEFAULT_INPUT_DIRECTORY.into());
            }
            config.validate()?;
            let archives =
                bfs::discovery::discover(&config.inputs, config.recursive, &config.output)?;
            let inspections = archives
                .iter()
                .map(|path| bfs::archive::inspect(path, &config.limits))
                .collect::<Result<Vec<_>>>()?;
            println!("{}", serde_json::to_string_pretty(&inspections)?);
            Ok(0)
        }
        Command::Stats { repository } => {
            let repo = bfs::storage::Repository::existing(&repository)?;
            let records = bfs::metadata::load(repo.root())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&bfs::report::summary(&repo, &records)?)?
            );
            Ok(0)
        }
        Command::Reorganize {
            repository,
            rules,
            dry_run,
        } => {
            let rules = bfs::rules::Rules::load(rules.as_deref())?;
            let repo = bfs::storage::Repository::existing(&repository)?;
            let records = bfs::metadata::load(repo.root())?;
            let plan = rules.plan(&records)?;
            if dry_run {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!("{}", bfs::organizer::organize(&repo, &rules, &plan)?);
            }
            Ok(0)
        }
    }
}
/// Initialize logging, run the CLI, and convert errors into an exit code.
fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    match run(Cli::parse()) {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::ExitCode::from(1)
        }
    }
}

fn cancellation() -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal = flag.clone();
    ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
    Ok(flag)
}
