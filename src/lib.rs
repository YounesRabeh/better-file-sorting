//! Exposes the BFS library modules used by the CLI and integration tests.
//! Acquisition produces verified local inputs; the existing pipeline handles extraction,
//! classification, content-addressed storage, rule planning and optional ZIP export.
//! Security and filesystem checks are shared across these boundaries.

pub mod archive;
pub mod classifier;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod error;
pub mod extraction;
pub mod metadata;
pub mod organizer;
pub mod pipeline;
pub mod report;
pub mod rules;
pub mod security;
pub mod storage;

pub mod acquisition;

pub mod http_source;

pub mod download;
