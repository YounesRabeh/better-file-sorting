use anyhow::Result;
use clap::Parser;
use malware_organizer::{
    cli::{Cli, Command},
    config::Config,
};
/// Dispatch one parsed CLI command and return its process exit code.
fn run(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Ingest(args) => {
            let config = Config::for_ingest(&args)?;
            if args.dry_run {
                malware_organizer::rules::Rules::load(config.rules.as_deref())?;
                let paths = malware_organizer::discovery::discover(
                    &config.inputs,
                    config.recursive,
                    &config.output,
                )?;
                let inspections = paths
                    .iter()
                    .map(|p| malware_organizer::archive::inspect(p, &config.limits))
                    .collect::<Result<Vec<_>>>()?;
                println!("{}", serde_json::to_string_pretty(&inspections)?);
                return Ok(0);
            }
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let signal = cancelled.clone();
            ctrlc::set_handler(move || signal.store(true, std::sync::atomic::Ordering::Relaxed))?;
            let report = malware_organizer::pipeline::ingest(&config, &cancelled)?;
            eprintln!(
                "Accepted {} samples; {} failures; {} unique objects ({:?})",
                report.accepted,
                report.failures,
                report.summary.unique_samples,
                report.summary.security_mode
            );
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
            let archives = malware_organizer::discovery::discover(
                &config.inputs,
                config.recursive,
                &config.output,
            )?;
            let inspections = archives
                .iter()
                .map(|path| malware_organizer::archive::inspect(path, &config.limits))
                .collect::<Result<Vec<_>>>()?;
            println!("{}", serde_json::to_string_pretty(&inspections)?);
            Ok(0)
        }
        Command::Stats { repository } => {
            let repo = malware_organizer::storage::Repository::existing(&repository)?;
            let records = malware_organizer::metadata::load(repo.root())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&malware_organizer::report::summary(
                    &repo, &records
                )?)?
            );
            Ok(0)
        }
        Command::Reorganize {
            repository,
            rules,
            dry_run,
        } => {
            let rules = malware_organizer::rules::Rules::load(Some(&rules))?;
            let repo = malware_organizer::storage::Repository::existing(&repository)?;
            let records = malware_organizer::metadata::load(repo.root())?;
            let plan = rules.plan(&records)?;
            if dry_run {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!(
                    "{}",
                    malware_organizer::organizer::organize(&repo, &rules, &plan)?
                );
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
