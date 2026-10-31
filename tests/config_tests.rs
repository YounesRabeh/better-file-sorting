// SPDX-License-Identifier: GPL-3.0-only
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
    Command::new(env!("CARGO_BIN_EXE_bfs"))
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
