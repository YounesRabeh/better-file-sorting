use anyhow::{Context, Result};
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
            let config = Config::load(Some(&args.config))?;
            let source = config
                .source
                .as_ref()
                .context("source configuration required")?;
            let download = config
                .download
                .as_ref()
                .context("download configuration required")?;
            let cancelled = cancellation()?;
            let inventory = bfs::http_source::discover(source, download, &cancelled)?;
            let parent = download
                .inventory
                .parent()
                .unwrap_or(std::path::Path::new("."));
            bfs::acquisition::private_tree(parent)?;
            bfs::acquisition::save_json(&download.inventory, &inventory)?;
            println!("{}", serde_json::to_string_pretty(&inventory)?);
            Ok(0)
        }
        Command::Download(_) => anyhow::bail!("download implementation pending"),
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
            let rules = bfs::rules::Rules::load(Some(&rules))?;
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
