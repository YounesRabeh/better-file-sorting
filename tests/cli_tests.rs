// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Integration checks for cli tests.
//! Exercises public BFS behavior with disposable fixtures; failures should identify
//! contract regressions without changing user repositories.

use std::process::Command;

#[test]
/// Ensure help exposes every top-level command.
fn help_lists_commands() {
    let out = Command::new(env!("CARGO_BIN_EXE_bfsort"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("Usage: bfsort"),
        "unexpected CLI name: {help}"
    );
    for command in ["ingest", "inspect", "stats", "reorganize"] {
        assert!(help.contains(command), "missing {command}");
    }
}

#[test]
/// Ensure invalid configuration fails before creating the output path.
fn rejects_invalid_configuration_before_output_creation() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "workers = 0\n").unwrap();
    let output = dir.path().join("repo");
    let status = Command::new(env!("CARGO_BIN_EXE_bfsort"))
        .args(["ingest", "--config"])
        .arg(config)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!status.status.success());
    assert!(!output.exists());
}

#[test]
fn discover_accepts_url_without_config() {
    use clap::Parser;
    assert!(bfs::cli::Cli::try_parse_from(["bfs", "discover", "https://example.org/"]).is_ok());
    assert!(
        bfs::cli::Cli::try_parse_from(["bfs", "discover", "--config", "collection.toml"]).is_ok()
    );
    assert!(bfs::cli::Cli::try_parse_from(["bfs", "discover"]).is_ok());
    assert!(bfs::cli::Cli::try_parse_from(["bfs", "download"]).is_ok());
}

#[test]
fn commands_accept_default_settings_without_config() {
    use clap::Parser;
    for command in ["download", "ingest", "inspect", "stats", "reorganize"] {
        assert!(
            bfs::cli::Cli::try_parse_from(["bfs", command]).is_ok(),
            "{command}"
        );
    }
}

#[test]
fn local_default_workflow_uses_incoming_and_organized() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("incoming")).unwrap();
    let file = std::fs::File::create(temp.path().join("incoming/example.zip")).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"hello").unwrap();
    zip.finish().unwrap();
    for args in [
        vec!["inspect"],
        vec!["ingest"],
        vec!["stats"],
        vec!["reorganize", "--dry-run"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_bfsort"))
            .args(&args)
            .current_dir(temp.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert!(temp.path().join("organized/security.json").is_file());
    assert!(temp.path().join("incoming/example.zip").is_file());
    assert!(!temp.path().join("repository").exists());
    assert!(!temp.path().join("downloads").exists());
}

#[test]
fn reworked_workflow_flags_parse() {
    use clap::Parser;
    for args in [
        vec![
            "bfs",
            "collect",
            "https://example.org/",
            "-o",
            "collection",
            "-j",
            "2",
            "--refresh",
            "--dry-run",
        ],
        vec![
            "bfs",
            "organize",
            "incoming",
            "-o",
            "organized",
            "--extract",
        ],
        vec!["bfs", "--no-color", "--quiet", "--json", "organize"],
        vec!["bfs", "config", "validate", "-c", "bfs.toml"],
    ] {
        assert!(bfs::cli::Cli::try_parse_from(&args).is_ok(), "{args:?}");
    }
}

#[test]
fn default_config_and_overrides_are_local() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("bfs.toml"), "workers = 0").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_bfsort"))
            .current_dir(temp.path())
            .args(args)
            .output()
            .unwrap()
    };
    let invalid = run(&["config", "validate"]);
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("workers"));
    let missing = run(&["config", "validate", "-c", "missing.toml"]);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("configuration file not found"));
    std::fs::write(temp.path().join("bfs.toml"), "workres = 1").unwrap();
    let invalid = run(&["config", "validate"]);
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("workres"));
}
