// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Runs the BFS command-line application and translates outcomes into process exit codes.
//! The dispatcher connects parsed arguments to configuration, cancellation and library
//! operations. Structured results go to stdout; diagnostics go to stderr so scripts can
//! consume reports without mixing them with progress and error messages.

use anyhow::Result;
use bfs::{
    app::cli::{Cli, Command},
    app::config::Config,
};
use clap::{CommandFactory, FromArgMatches};
/// Dispatch one parsed CLI command and return its process exit code.
fn run(cli: Cli) -> Result<u8> {
    let ui = &cli.presentation;
    bfs::app::presentation::initialize(ui);
    let started = std::time::Instant::now();
    match cli.command {
        Command::Config {
            command: bfs::app::cli::ConfigCommand::Validate { config },
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
                let inventory =
                    bfs::collection::http_source::discover(&source, &download, &cancelled)?;
                // A dry run never publishes acquisition state or creates payload storage.
                if !args.dry_run {
                    save_inventory(&download, &inventory)?;
                }
                inventory
            } else {
                bfs::collection::acquisition::load_inventory(&download.inventory)?
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
            let inventory = bfs::collection::http_source::discover(&source, &download, &cancelled)?;
            save_inventory(&download, &inventory)?;
            let files: Vec<_> = inventory.candidates.iter().map(|c| &c.name).collect();
            if ui.json && !ui.quiet {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({"files": files}))?
                );
            } else if !ui.quiet {
                for name in files {
                    println!("{}", bfs::app::presentation::safe_text(name));
                }
            }
            Ok(0)
        }
        Command::Download(args) => {
            let config = load(args.config.as_deref(), ui)?;
            let download = config.download.unwrap_or_default();
            let inventory = bfs::collection::acquisition::load_inventory(&download.inventory)?;
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
        Command::Ingest(args) => {
            log_config(args.config.as_deref(), ui);
            let config = Config::for_ingest(&args)?;
            if ui.verbose {
                effective(&config);
            }
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let signal = cancelled.clone();
            ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
            if args.dry_run {
                let preview = bfs::ingest::pipeline::preview(&config, &cancelled)?;
                output(
                    ui,
                    &preview,
                    &format!(
                        "{}\nPreview: {} occurrences; {} unique; {} duplicates; output {}",
                        preview
                            .plan
                            .iter()
                            .map(|e| bfs::app::presentation::safe_text(&e.planned.path))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        preview.summary.occurrences,
                        preview.summary.unique_files,
                        preview.summary.duplicate_occurrences,
                        bfs::app::presentation::safe_text(&config.output.display().to_string())
                    ),
                )?;
                return Ok(0);
            }
            if config.archive.nested_extraction
                && (config.archive.max_total_entries.is_none()
                    || config.archive.max_total_expanded_bytes.is_none())
                && !ui.quiet
                && !ui.json
            {
                eprintln!(
                    "Warning: recursive extraction has unlimited tree budgets; configure [archive] limits."
                );
            }
            let report = bfs::ingest::pipeline::ingest(&config, &cancelled)?;
            output(
                ui,
                &report,
                &format!(
                    "{}: {} accepted occurrences; {} reused occurrences; {} skipped files; {} failed; {}; {:.2}s; output {}{}",
                    if report.failures > 0 || report.interrupted {
                        "partial"
                    } else {
                        "done"
                    },
                    report.accepted,
                    report.reused_occurrences,
                    report.skipped_files,
                    report.failures,
                    bfs::app::presentation::bytes(report.summary.logical_bytes),
                    report.elapsed_seconds,
                    bfs::app::presentation::safe_text(&config.output.display().to_string()),
                    if report.interrupted {
                        "; interrupted"
                    } else {
                        ""
                    }
                ),
            )?;
            if let Some(error) = &report.first_failure {
                eprintln!(
                    "error: {}: {}",
                    bfs::app::presentation::safe_text(&error.source_path),
                    bfs::app::presentation::safe_text(&error.error)
                );
            }
            if report.interrupted {
                eprintln!(
                    "Interrupted: {} samples accepted; {} failures",
                    report.accepted, report.failures
                );
            }
            Ok(if report.failures > 0 || report.interrupted {
                if report.accepted + report.reused_occurrences > 0 {
                    2
                } else {
                    1
                }
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
                    .push(bfs::app::config::DEFAULT_INPUT_DIRECTORY.into());
            }
            config.validate()?;
            let archives =
                bfs::ingest::discovery::discover(&config.inputs, config.recursive, &config.output)?;
            let inspections = archives
                .iter()
                .map(|path| bfs::output::archive::inspect(path, &config.limits))
                .collect::<Result<Vec<_>>>()?;
            output(
                ui,
                &inspections,
                &serde_json::to_string_pretty(&inspections)?,
            )?;
            Ok(0)
        }
        Command::Report {
            command: bfs::app::cli::ReportCommand::Mismatches(args),
        } => {
            let repo = bfs::repository::storage::Repository::existing(&args.repository)?;
            let records = bfs::repository::metadata::load(repo.root())?;
            let report = bfs::output::report::mismatches(
                &records,
                &args.status,
                args.min_confidence,
                args.all,
            );
            if report.rows.is_empty()
                && !ui.quiet
                && !ui.json
                && matches!(args.format, bfs::output::report::ReportFormat::Table)
            {
                if records.is_empty() {
                    eprintln!("No stored occurrences to report.");
                } else if report.counts.get("not_evaluated").copied().unwrap_or(0) > 0 {
                    eprintln!(
                        "Some records were not evaluated; use analyze explicitly on an accessible standard repository."
                    );
                } else if report.counts.get("mismatch").copied().unwrap_or(0) == 0 {
                    eprintln!(
                        "No definitive mismatches. Use --all or --status to inspect other statuses."
                    );
                } else {
                    eprintln!(
                        "All rows were filtered out. Use --all or adjust --status and --min-confidence."
                    );
                }
            }
            let format =
                if ui.json && matches!(args.format, bfs::output::report::ReportFormat::Table) {
                    bfs::output::report::ReportFormat::Json
                } else {
                    args.format
                };
            if let Some(path) = args.output {
                bfs::ingest::discovery::check_path(&path)?;
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(std::path::Path::new("."));
                let mut file = tempfile::NamedTempFile::new_in(parent)?;
                report.write(&mut file, format)?;
                file.as_file().sync_all()?;
                file.persist_noclobber(&path)?;
                std::fs::File::open(parent)?.sync_all()?;
            } else if !ui.quiet || ui.json {
                report.write(std::io::stdout().lock(), format)?;
            }
            Ok(0)
        }
        Command::Analyze {
            repository,
            config,
            resume,
            dry_run,
        } => {
            let repo = bfs::repository::storage::Repository::existing(&repository)?;
            let mut config = load(config.as_deref(), ui)?;
            config.resume = resume;
            config.input_mode = bfs::app::config::InputMode::Archives;
            config.validate()?;
            if dry_run {
                let preview = bfs::ingest::nested::preview(&repo, &config)?;
                output(
                    ui,
                    &preview,
                    &format!(
                        "Preview: {} archive occurrences; {} unique; {} reusable; {} need extraction; expanded size unknown",
                        preview.candidate_occurrences,
                        preview.unique_archives,
                        preview.reusable_archives,
                        preview.needs_extraction
                    ),
                )?;
                return Ok(0);
            }
            if (config.archive.max_total_entries.is_none()
                || config.archive.max_total_expanded_bytes.is_none())
                && !ui.quiet
                && !ui.json
            {
                eprintln!(
                    "Warning: recursive extraction has unlimited tree budgets; configure [archive] limits."
                );
            }
            let report = bfs::ingest::nested::analyze(&repo, &config, cancellation()?.as_ref())?;
            output(
                ui,
                &report,
                &format!(
                    "Analyzed: {} child occurrences; {} failed; {} archives extracted; {} reused",
                    report.accepted,
                    report.failures,
                    report.metrics.extracted_archives,
                    report.metrics.reused_archives
                ),
            )?;
            Ok(if report.failures > 0 { 1 } else { 0 })
        }
        Command::Reclassify {
            repository,
            resume,
            dry_run,
        } => {
            let repo = bfs::repository::storage::Repository::existing(&repository)?;
            if dry_run {
                let rows = bfs::repository::metadata::load(repo.root())?;
                let hashes: std::collections::BTreeSet<_> =
                    rows.iter().map(|r| &r.sha256).collect();
                output(
                    ui,
                    &serde_json::json!({"unique_objects": hashes.len(), "detector_version": bfs::detection::classifier::DETECTOR_VERSION}),
                    &format!(
                        "Preview: {} unique objects; detector {}",
                        hashes.len(),
                        bfs::detection::classifier::DETECTOR_VERSION
                    ),
                )?;
                return Ok(0);
            }
            let count =
                bfs::repository::metadata::reclassify(&repo, cancellation()?.as_ref(), resume)?;
            output(
                ui,
                &serde_json::json!({"analyzed_occurrences": count}),
                &format!("Reclassified: {count} occurrences"),
            )?;
            Ok(0)
        }
        Command::Export(args) => {
            let repo = bfs::repository::storage::Repository::existing(&args.repository)?;
            let rules = bfs::organization::rules::Rules::load_or_preset(
                args.rules.as_deref(),
                args.preset,
            )?;
            let config = Config {
                resume: args.resume,
                output: args.output,
                zip: true,
                zip_each_dir: args.zip_each_dir,
                zip_dirs: args.zip_dirs,
                output_password_file: args.output_password_file,
                ..Config::default()
            };
            let exports = bfs::output::archive::export_repository(
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
                    "{}: {} ZIP collections; {} reused; {:.2}s; output {}",
                    if args.dry_run { "Preview" } else { "done" },
                    exports.len(),
                    exports.iter().filter(|export| export.reused).count(),
                    started.elapsed().as_secs_f64(),
                    bfs::app::presentation::safe_text(&config.output.display().to_string())
                ),
            )?;
            Ok(0)
        }
        Command::Stats { repository } => {
            let repo = bfs::repository::storage::Repository::existing(&repository)?;
            let records = bfs::repository::metadata::load(repo.root())?;
            let summary = bfs::output::report::summary(&repo, &records)?;
            output(
                ui,
                &summary,
                &bfs::app::presentation::stats(repo.root(), &summary),
            )?;
            Ok(0)
        }
        Command::Reorganize {
            repository,
            rules,
            preset,
            dry_run,
        } => {
            let rules = bfs::organization::rules::Rules::load_or_preset(rules.as_deref(), preset)?;
            let repo = bfs::repository::storage::Repository::existing(&repository)?;
            let records = bfs::repository::metadata::load(repo.root())?;
            let plan = rules.plan(&records)?;
            if dry_run {
                output(ui, &plan, &serde_json::to_string_pretty(&plan)?)?;
            } else {
                let count = bfs::organization::organizer::organize(&repo, &rules, &plan)?;
                output(ui, &count, &format!("Completed: {count} organized"))?;
            }
            Ok(0)
        }
    }
}
/// Initialize logging, run the CLI, and convert errors into an exit code.
fn main() -> std::process::ExitCode {
    let arguments: Vec<_> = std::env::args_os().collect();
    let mut mode = bfs::app::cli::ColorMode::Auto;
    for pair in arguments.windows(2) {
        if pair[0] == "--color" {
            mode = match pair[1].to_str() {
                Some("always") => bfs::app::cli::ColorMode::Always,
                Some("never") => bfs::app::cli::ColorMode::Never,
                _ => bfs::app::cli::ColorMode::Auto,
            };
        }
    }
    for arg in &arguments {
        if arg == "--color=always" {
            mode = bfs::app::cli::ColorMode::Always;
        }
        if arg == "--color=never" {
            mode = bfs::app::cli::ColorMode::Never;
        }
    }
    let machine_format = arguments.windows(2).any(|pair| {
        pair[0] == "--format" && matches!(pair[1].to_str(), Some("json" | "jsonl" | "csv"))
    }) || arguments.iter().any(|a| {
        matches!(
            a.to_str(),
            Some("--format=json" | "--format=jsonl" | "--format=csv")
        )
    });
    let disabled = machine_format || arguments.iter().any(|a| a == "--no-color" || a == "--json");
    let choice = if disabled {
        clap::ColorChoice::Never
    } else {
        match mode {
            bfs::app::cli::ColorMode::Always => clap::ColorChoice::Always,
            bfs::app::cli::ColorMode::Never => clap::ColorChoice::Never,
            bfs::app::cli::ColorMode::Auto => clap::ColorChoice::Auto,
        }
    };
    let matches = Cli::command().color(choice).get_matches_from(arguments);
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    if matches!(&cli.command, Command::Report { command: bfs::app::cli::ReportCommand::Mismatches(args) } if !matches!(args.format, bfs::output::report::ReportFormat::Table))
    {
        cli.presentation.json = true;
    }
    bfs::app::presentation::initialize(&cli.presentation);
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
            eprintln!(
                "{}: {}",
                bfs::app::presentation::label("error", "31"),
                bfs::app::presentation::safe_text(&format!("{error:#}"))
            );
            if verbose {
                eprintln!(
                    "Details: {}",
                    bfs::app::presentation::safe_text(&format!("{error:#}"))
                );
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

fn log_config(path: Option<&std::path::Path>, ui: &bfs::app::cli::Presentation) {
    if ui.verbose {
        match Config::active_path(path) {
            Some(path) => eprintln!(
                "Configuration: {}",
                bfs::app::presentation::safe_text(&path.display().to_string())
            ),
            None => eprintln!("Configuration: built-in defaults"),
        }
    }
}
fn load(path: Option<&std::path::Path>, ui: &bfs::app::cli::Presentation) -> Result<Config> {
    log_config(path, ui);
    let config = Config::load(path)?;
    if ui.verbose {
        effective(&config);
    }
    Ok(config)
}
fn effective(config: &Config) {
    eprintln!(
        "Settings: output {}; workers {}; resume {}",
        bfs::app::presentation::safe_text(&config.output.display().to_string()),
        config.workers,
        config.resume
    );
    let size = |value: Option<u64>| {
        value
            .map(bfs::app::presentation::bytes)
            .unwrap_or_else(|| "unlimited".into())
    };
    eprintln!(
        "Limits: entry {}; archive {}; input {}; entries {}; expansion ratio {}",
        size(config.limits.max_entry_bytes),
        size(config.limits.max_archive_bytes),
        size(config.limits.max_input_bytes),
        config
            .limits
            .max_entries
            .map_or_else(|| "unlimited".into(), |n| n.to_string()),
        config
            .limits
            .max_expansion_ratio
            .map_or_else(|| "unlimited".into(), |n| n.to_string())
    );
    for path in &config.inputs {
        eprintln!(
            "Input: {}",
            bfs::app::presentation::safe_text(&path.display().to_string())
        );
    }
    if let Some(source) = &config.source {
        eprintln!(
            "Discovery: page {}; timeout {}",
            size(source.max_page_size),
            source
                .timeout_seconds
                .map_or_else(|| "disabled".into(), |n| format!("{n}s"))
        );
    }
    if let Some(download) = &config.download {
        eprintln!(
            "Download budgets: file {}; total {}; staging {}; files {}; requests {}",
            size(download.max_file_size),
            size(download.max_total_size),
            size(download.max_staging_size),
            download
                .max_files
                .map_or_else(|| "unlimited".into(), |n| n.to_string()),
            download
                .max_requests
                .map_or_else(|| "unlimited".into(), |n| n.to_string())
        );
        eprintln!(
            "Download: inventory {}; staging {}; workers {}; timeout {}",
            bfs::app::presentation::safe_text(&download.inventory.display().to_string()),
            bfs::app::presentation::safe_text(&download.staging_dir.display().to_string()),
            download.concurrency,
            download
                .timeout_seconds
                .map_or_else(|| "disabled".into(), |n| format!("{n}s"))
        );
    }
}
fn output(
    ui: &bfs::app::cli::Presentation,
    value: &impl serde::Serialize,
    human: &str,
) -> Result<()> {
    if ui.quiet {
        return Ok(());
    }
    if ui.json {
        println!("{}", serde_json::to_string(value)?);
    } else if !ui.quiet {
        use std::io::IsTerminal;
        if bfs::app::presentation::color_enabled(
            ui.color,
            ui.no_color,
            ui.json,
            std::io::stdout().is_terminal(),
            std::env::var_os("NO_COLOR").is_some(),
        ) {
            for line in human.lines() {
                let boundary = line.find(char::is_whitespace).unwrap_or(line.len());
                let (label, rest) = line.split_at(boundary);
                let code = match label.trim_end_matches(':') {
                    "partial" => Some("33"),
                    "done" | "Completed" => Some("32"),
                    "Preview" | "Repository" | "Samples" | "Duplicates" | "Stored" | "Logical"
                    | "Mismatches" | "Detected" | "Mismatch" | "Configuration" => Some("36"),
                    _ => None,
                };
                if let Some(code) = code {
                    println!("\x1b[1;{code}m{label}\x1b[0m{rest}");
                } else {
                    println!("{line}");
                }
            }
        } else {
            println!("{human}");
        }
    }
    Ok(())
}
fn save_inventory(
    download: &bfs::collection::acquisition::DownloadConfig,
    inventory: &bfs::collection::acquisition::Inventory,
) -> Result<()> {
    let parent = download
        .inventory
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    bfs::collection::acquisition::private_tree(parent)?;
    bfs::collection::acquisition::save_json(&download.inventory, inventory)
}
fn download_inventory(
    ui: &bfs::app::cli::Presentation,
    download: &bfs::collection::acquisition::DownloadConfig,
    mut source: bfs::collection::acquisition::SourceConfig,
    inventory: &bfs::collection::acquisition::Inventory,
    dry_run: bool,
    cancelled: &std::sync::atomic::AtomicBool,
    started: std::time::Instant,
) -> Result<u8> {
    let report = if dry_run || inventory.candidates.is_empty() {
        bfs::collection::download::preview(download, inventory)?
    } else {
        if source.page.is_empty() {
            source.page = inventory.candidates[0].url.clone();
        }
        bfs::collection::download::run(download, &source, inventory, cancelled)?
    };
    output(
        ui,
        &report,
        &format!(
            "{}: {} downloaded; {} cached; {} failed; {} pending; {} selected; {} filtered; {}; {:.2}s; output {}",
            if dry_run {
                "Preview"
            } else if report.failed + report.not_attempted > 0 {
                "partial"
            } else {
                "done"
            },
            report.downloaded,
            report.cached,
            report.failed,
            report.not_attempted,
            report.candidates.len(),
            report.filtered,
            bfs::app::presentation::bytes(report.bytes),
            started.elapsed().as_secs_f64(),
            bfs::app::presentation::safe_text(&download.staging_dir.display().to_string())
        ),
    )?;
    if !ui.quiet && !ui.json && !dry_run {
        eprintln!(
            "hint: downloads are not an ingested repository. With configured receipts and authorized payload access, use bfsort ingest --extract; otherwise specify accessible input paths explicitly."
        );
    }
    for error in &report.errors {
        eprintln!("error: {}", bfs::app::presentation::safe_text(error));
    }
    Ok(if report.failed + report.not_attempted == 0 {
        0
    } else if report.downloaded + report.cached > 0 {
        2
    } else {
        1
    })
}
