// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Integration checks for config tests.
//! Exercises public BFS behavior with disposable fixtures; failures should identify
//! contract regressions without changing user repositories.

use std::process::Command;
/// Run ingest with a temporary configuration file.
fn ingest(config: &str, args: &[&str]) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_bfsort"))
        .args(["ingest", "--config"])
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}
#[test]
/// Ensure unknown and unsafe configuration values are rejected.
fn rejects_unknown_fields_and_unsafe_security_settings() {
    for config in [
        "workres = 2",
        "[security]\nmode = 'public'",
        "[security]\nallow_automatic_unsealing = true",
        "[security]\nrestrict_directories = false",
        "[limits]\nmax_entry_bytes = 0",
    ] {
        let out = ingest(config, &[]);
        assert!(!out.status.success(), "accepted {config}");
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("not implemented"),
            "configuration was not validated: {config}"
        );
    }
}
#[test]
/// Ensure explicit worker flags override configuration values.
fn cli_workers_override_config() {
    let out = ingest("workers = 0", &["--workers", "2"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("workers must"), "{err}");
}

#[test]
fn extraction_limits_accept_readable_sizes_and_legacy_bytes() {
    let config: bfs::config::Config = toml::from_str("[limits]\nmax_entry_bytes = '256 MiB'\nmax_archive_bytes = '1 GiB'\nmax_input_bytes = 1073741824").unwrap();
    config.validate().unwrap();
    assert_eq!(config.limits.max_entry_bytes, 268435456);
    assert_eq!(config.limits.max_archive_bytes, 1073741824);
    assert_eq!(config.limits.max_input_bytes, 1073741824);
    for value in [
        "'0 MB'",
        "'-1 MB'",
        "'1.5 MB'",
        "'18446744073709551615 GB'",
        "-1",
        "1.5",
    ] {
        assert!(
            toml::from_str::<bfs::config::Config>(&format!("[limits]\nmax_entry_bytes = {value}"))
                .is_err()
        );
    }
    let example: bfs::config::Config =
        toml::from_str(include_str!("../examples/config.example.toml")).unwrap();
    example.validate().unwrap();
}

#[test]
fn readable_units_and_numeric_fallback_are_shared() {
    let config: bfs::config::Config = toml::from_str("[source]\nmax_page_size = 5000000\n[download]\nmax_file_size = 1000\nmax_total_size = '2 MiB'\nmax_staging_size = 3000000\nmax_speed = 2000000").unwrap();
    config.download.as_ref().unwrap().validate().unwrap();
    assert_eq!(config.download.unwrap().speed().unwrap(), Some(2_000_000));
    let matcher: bfs::rules::Matcher =
        toml::from_str("min_size = '1 KB'\nmax_size = '2 MiB'").unwrap();
    assert_eq!(matcher.min_size, Some(1000));
    assert_eq!(matcher.max_size, Some(2_097_152));
    for value in ["'2 MB/s'", "'2 MiB/s'"] {
        let config: bfs::acquisition::DownloadConfig =
            toml::from_str(&format!("max_speed = {value}")).unwrap();
        assert!(config.speed().unwrap().is_some());
    }
}
