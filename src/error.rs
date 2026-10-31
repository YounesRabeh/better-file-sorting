// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Defines structured configuration errors exposed by the library and command dispatcher.
//! These errors distinguish invalid user settings from lower-level filesystem, archive
//! and transfer failures, which retain context through anyhow at their call sites.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(String),
}
