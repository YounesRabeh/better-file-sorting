// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Integration checks for platform tests.
//! Exercises public BFS behavior with disposable fixtures; failures should identify
//! contract regressions without changing user repositories.

#[cfg(not(unix))]
#[test]
/// Ensure unsupported platforms fail closed instead of weakening storage.
fn storage_fails_closed_without_acl_backend() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    assert!(bfs::storage::Repository::open(&root, bfs::config::SecurityMode::Sealed).is_err());
    assert!(!root.exists());
}
#[test]
/// Ensure shipped example configuration files remain parseable.
fn example_configuration_and_rules_are_valid() {
    let config: bfs::config::Config =
        toml::from_str(include_str!("../config.example.toml")).unwrap();
    config.validate().unwrap();
    bfs::rules::Rules::parse(include_str!("../rules.example.toml")).unwrap();
    bfs::rules::Rules::parse(include_str!("../rules.files.example.toml")).unwrap();
}
