// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Materializes rule-planned repository views without changing canonical stored objects.
//! View construction uses private staging and validates destination paths before
//! publication. Metadata-only organization can operate on sealed repositories without
//! unlocking payloads, while existing view generations remain available.

use crate::{
    discovery::check_path,
    rules::{Planned, Rules, ViewMode},
    security,
    storage::Repository,
};
use anyhow::{Context, Result};
use std::{
    fs::{self, File},
    io::Write,
};

/// Publish an immutable generation, then atomically switch a small pointer file.
/// Previous generations are deliberately retained for recovery.
/// Build a view from metadata without reading canonical payload contents.
pub fn organize(repo: &Repository, rules: &Rules, plan: &[Planned]) -> Result<String> {
    let views = repo.root().join("views");
    security::private_dir(&views)?;
    let staging = security::private_tempdir(&repo.staging(), "view-")?;
    let generation = staging
        .path()
        .file_name()
        .expect("view name")
        .to_string_lossy()
        .into_owned();
    for entry in plan {
        let source = repo.object(&entry.sha256)?;
        security::validate_file(&source, repo.mode())?;
        if rules.organization.view_mode == ViewMode::Manifest {
            continue;
        }
        let destination = staging.path().join(&entry.path);
        check_path(&destination)?;
        let mut directory = staging.path().to_path_buf();
        for part in std::path::Path::new(&entry.path)
            .parent()
            .expect("planned parent")
            .components()
        {
            anyhow::ensure!(
                matches!(part, std::path::Component::Normal(_)),
                "unsafe planned view path"
            );
            directory.push(part);
            security::private_dir(&directory)?;
        }
        fs::hard_link(&source, &destination)
            .context("hard link failed; use view_mode = 'manifest' for this filesystem")?;
        security::validate_file(&destination, repo.mode())?;
    }
    let mut index = tempfile::NamedTempFile::new_in(staging.path())?;
    for entry in plan {
        serde_json::to_writer(&mut index, entry)?;
        index.write_all(b"\n")?;
    }
    index.as_file().sync_all()?;
    index.persist_noclobber(staging.path().join("layout.jsonl"))?;
    for entry in walkdir::WalkDir::new(staging.path())
        .contents_first(true)
        .follow_links(false)
    {
        let entry = entry?;
        if entry.file_type().is_dir() {
            File::open(entry.path())?.sync_all()?;
        }
    }
    let target = views.join(&generation);
    // A random private generation is never reused or overwritten.
    anyhow::ensure!(!target.try_exists()?, "view generation already exists");
    fs::rename(staging.path(), &target)?;
    File::open(&views)?.sync_all()?;
    let pointer = views.join("CURRENT");
    check_path(&pointer)?;
    if pointer.exists() {
        security::validate_file(&pointer, crate::config::SecurityMode::Standard)?;
    }
    let mut file = tempfile::NamedTempFile::new_in(&views)?;
    writeln!(file, "{generation}")?;
    file.as_file().sync_all()?;
    file.persist(&pointer)?;
    File::open(&views)?.sync_all()?;
    Ok(generation)
}
