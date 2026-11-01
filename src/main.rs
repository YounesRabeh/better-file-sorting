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
    let ui = &cli.presentation;
    bfs::presentation::initialize(ui);
    let started = std::time::Instant::now();
    match cli.command {
        Command::Config {
            command: bfs::cli::ConfigCommand::Validate { config },
        } => {
            let config = load(config.as_deref(), ui)?;
            config.validate()?;
            output(
                ui,
                &serde_json::json!({"valid": true}),
                "Configuration valid",
            )?;
            Ok(0)
        }
        Command::Collect(args) => {
            let config = load(args.config.as_deref(), ui)?;
            let mut source = config.source.unwrap_or_default();
            if let Some(url) = args.url {
                source.page = url;
            }
            let mut download = config.download.unwrap_or_default();
            if let Some(root) = args.output {
                download.inventory = root.join("inventory.jsonl");
                download.staging_dir = root.join("downloads");
            }
            if let Some(workers) = args.workers {
                download.concurrency = workers;
            }
            download.validate()?;
            let cancelled = cancellation()?;
            let inventory = if args.refresh || !download.inventory.try_exists()? {
                if args.dry_run && !ui.quiet && !ui.json {
                    eprintln!("Preview: requesting HTML metadata; no payloads will be downloaded.");
                }
                let inventory = bfs::http_source::discover(&source, &download, &cancelled)?;
                // A dry run never publishes acquisition state or creates payload storage.
                if !args.dry_run {
                    save_inventory(&download, &inventory)?;
                }
                inventory
            } else {
                bfs::acquisition::load_inventory(&download.inventory)?
            };
            download_inventory(
                ui,
                &download,
                source,
                &inventory,
                args.dry_run,
                &cancelled,
                started,
            )
        }
        Command::Discover(args) => {
            anyhow::ensure!(
                !args.dry_run,
                "discover performs HTTP requests; use download --dry-run for offline preview"
            );
            let config = load(args.config.as_deref(), ui)?;
            let mut source = config.source.unwrap_or_default();
            if let Some(url) = args.url {
                source.page = url;
            }
            let download = config.download.unwrap_or_default();
            let cancelled = cancellation()?;
            let inventory = bfs::http_source::discover(&source, &download, &cancelled)?;
            save_inventory(&download, &inventory)?;
            let files: Vec<_> = inventory.candidates.iter().map(|c| &c.name).collect();
            if ui.json {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({"files": files}))?
                );
            } else if !ui.quiet {
                for name in files {
                    println!("{}", bfs::presentation::safe_text(name));
                }
            }
            Ok(0)
        }
        Command::Download(args) => {
            let config = load(args.config.as_deref(), ui)?;
            let download = config.download.unwrap_or_default();
            let inventory = bfs::acquisition::load_inventory(&download.inventory)?;
            download_inventory(
                ui,
                &download,
                config.source.unwrap_or_default(),
                &inventory,
                args.dry_run,
                cancellation()?.as_ref(),
                started,
            )
        }
        Command::Ingest(args) | Command::Organize(args) => {
            log_config(args.config.as_deref(), ui);
            let config = Config::for_ingest(&args)?;
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let signal = cancelled.clone();
            ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
            if args.dry_run {
                let preview = bfs::pipeline::preview(&config, &cancelled)?;
                output(
                    ui,
                    &preview,
                    &format!(
                        "{}\nPreview: {} occurrences; {} unique; {} duplicates; output {}",
                        preview
                            .plan
                            .iter()
                            .map(|e| e.planned.path.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        preview.summary.occurrences,
                        preview.summary.unique_files,
                        preview.summary.duplicate_occurrences,
                        config.output.display()
                    ),
                )?;
                return Ok(0);
            }
            let report = bfs::pipeline::ingest(&config, &cancelled)?;
            output(
                ui,
                &report,
                &format!(
                    "Completed: {} samples; {} failed; {} bytes; {:.2}s; output {}{}",
                    report.accepted,
                    report.failures,
                    report.summary.logical_bytes,
                    report.elapsed_seconds,
                    config.output.display(),
                    if report.interrupted {
                        "; interrupted"
                    } else {
                        ""
                    }
                ),
            )?;
            if let Some(error) = &report.first_failure {
                eprintln!("Error: {}: {}", error.source_path, error.error);
            }
            if report.interrupted {
                eprintln!(
                    "Interrupted: {} samples accepted; {} failures",
                    report.accepted, report.failures
                );
            }
            Ok(if report.failures > 0 || report.interrupted {
                if report.accepted > 0 { 2 } else { 1 }
            } else {
                0
            })
        }
        Command::Inspect(args) => {
            let mut config = load(args.config.as_deref(), ui)?;
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
            output(
                ui,
                &inspections,
                &serde_json::to_string_pretty(&inspections)?,
            )?;
            Ok(0)
        }
        Command::Report {
            command: bfs::cli::ReportCommand::Mismatches(args),
        } => {
            let repo = bfs::storage::Repository::existing(&args.repository)?;
            let records = bfs::metadata::load(repo.root())?;
            let report =
                bfs::report::mismatches(&records, &args.status, args.min_confidence, args.all);
            let format = if ui.json && matches!(args.format, bfs::report::ReportFormat::Table) {
                bfs::report::ReportFormat::Json
            } else {
                args.format
            };
            if let Some(path) = args.output {
                bfs::discovery::check_path(&path)?;
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(std::path::Path::new("."));
                let mut file = tempfile::NamedTempFile::new_in(parent)?;
                report.write(&mut file, format)?;
                file.as_file().sync_all()?;
                file.persist_noclobber(&path)?;
                std::fs::File::open(parent)?.sync_all()?;
            } else if !ui.quiet {
                report.write(std::io::stdout().lock(), format)?;
            }
            Ok(0)
        }
        Command::Analyze { repository } => {
            let repo = bfs::storage::Repository::existing(&repository)?;
            let count = bfs::metadata::analyze(&repo, cancellation()?.as_ref())?;
            output(
                ui,
                &serde_json::json!({"analyzed_occurrences": count}),
                &format!("Analyzed: {count} occurrences"),
            )?;
            Ok(0)
        }
        Command::Export(args) => {
            let repo = bfs::storage::Repository::existing(&args.repository)?;
            let rules = bfs::rules::Rules::load(args.rules.as_deref())?;
            let config = Config {
                output: args.output,
                zip: true,
                zip_each_dir: args.zip_each_dir,
                zip_dirs: args.zip_dirs,
                output_password_file: args.output_password_file,
                ..Config::default()
            };
            let exports = bfs::archive::export_repository(
                &repo,
                &rules,
                &config,
                args.dry_run,
                cancellation()?.as_ref(),
            )?;
            output(
                ui,
                &exports,
                &format!(
                    "{}: {} ZIP collections",
                    if args.dry_run { "Preview" } else { "Exported" },
                    exports.len()
                ),
            )?;
            Ok(0)
        }
        Command::Stats { repository } => {
            let repo = bfs::storage::Repository::existing(&repository)?;
            let records = bfs::metadata::load(repo.root())?;
            let summary = bfs::report::summary(&repo, &records)?;
            output(ui, &summary, &serde_json::to_string_pretty(&summary)?)?;
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
                output(ui, &plan, &serde_json::to_string_pretty(&plan)?)?;
            } else {
                let count = bfs::organizer::organize(&repo, &rules, &plan)?;
                output(ui, &count, &format!("Completed: {count} organized"))?;
            }
            Ok(0)
        }
    }
}
/// Initialize logging, run the CLI, and convert errors into an exit code.
fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    bfs::presentation::initialize(&cli.presentation);
    let verbose = cli.presentation.verbose;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    match run(cli) {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("{}: {error:#}", bfs::presentation::label("Error", "31"));
            if verbose {
                eprintln!("Details: {error:#}");
            }
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

fn log_config(path: Option<&std::path::Path>, ui: &bfs::cli::Presentation) {
    if ui.verbose {
        match Config::active_path(path) {
            Some(path) => eprintln!("Configuration: {}", path.display()),
            None => eprintln!("Configuration: built-in defaults"),
        }
    }
}
fn load(path: Option<&std::path::Path>, ui: &bfs::cli::Presentation) -> Result<Config> {
    log_config(path, ui);
    Config::load(path)
}
fn output(ui: &bfs::cli::Presentation, value: &impl serde::Serialize, human: &str) -> Result<()> {
    if ui.json {
        println!("{}", serde_json::to_string(value)?);
    } else if !ui.quiet {
        use std::io::IsTerminal;
        if bfs::presentation::color_enabled(
            ui.color,
            ui.no_color,
            ui.json,
            std::io::stdout().is_terminal(),
            std::env::var_os("NO_COLOR").is_some(),
        ) {
            println!("\x1b[32m{human}\x1b[0m");
        } else {
            println!("{human}");
        }
    }
    Ok(())
}
fn save_inventory(
    download: &bfs::acquisition::DownloadConfig,
    inventory: &bfs::acquisition::Inventory,
) -> Result<()> {
    let parent = download
        .inventory
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    bfs::acquisition::private_tree(parent)?;
    bfs::acquisition::save_json(&download.inventory, inventory)
}
fn download_inventory(
    ui: &bfs::cli::Presentation,
    download: &bfs::acquisition::DownloadConfig,
    mut source: bfs::acquisition::SourceConfig,
    inventory: &bfs::acquisition::Inventory,
    dry_run: bool,
    cancelled: &std::sync::atomic::AtomicBool,
    started: std::time::Instant,
) -> Result<u8> {
    let report = if dry_run || inventory.candidates.is_empty() {
        bfs::download::preview(download, inventory)?
    } else {
        if source.page.is_empty() {
            source.page = inventory.candidates[0].url.clone();
        }
        bfs::download::run(download, &source, inventory, cancelled)?
    };
    output(
        ui,
        &report,
        &format!(
            "{}: {} downloaded; {} cached; {} failed; {} pending; {} selected; {} filtered; {} bytes; {:.2}s; output {}",
            if dry_run { "Preview" } else { "Completed" },
            report.downloaded,
            report.cached,
            report.failed,
            report.not_attempted,
            report.candidates.len(),
            report.filtered,
            report.bytes,
            started.elapsed().as_secs_f64(),
            download.staging_dir.display()
        ),
    )?;
    for error in &report.errors {
        eprintln!("Error: {error}");
    }
    Ok(if report.failed + report.not_attempted == 0 {
        0
    } else if report.downloaded + report.cached > 0 {
        2
    } else {
        1
    })
}
