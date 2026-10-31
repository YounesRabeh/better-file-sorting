// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Integration checks for download tests.
//! Exercises public BFS behavior with disposable fixtures; failures should identify
//! contract regressions without changing user repositories.

use bfs::{acquisition::*, download};
#[test]
fn dry_run_never_creates_staging() {
    let temp = tempfile::tempdir().unwrap();
    let config = DownloadConfig {
        staging_dir: temp.path().join("absent"),
        ..Default::default()
    };
    let inventory = Inventory {
        version: 1,
        candidates: vec![],
    };
    assert_eq!(
        download::preview(&config, &inventory).unwrap().downloaded,
        0
    );
    assert!(!config.staging_dir.exists());
}

fn transfer(
    body: &'static [u8],
    limit: &str,
) -> (tempfile::TempDir, DownloadConfig, DownloadReport) {
    transfer_with_speed(body, limit, None)
}
fn transfer_with_speed(
    body: &'static [u8],
    limit: &str,
    speed: Option<String>,
) -> (tempfile::TempDir, DownloadConfig, DownloadReport) {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::AtomicBool,
    };
    let root = tempfile::tempdir().unwrap();
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/a.zip", server.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).unwrap() > 0);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        let _ = stream.write_all(body);
    });
    let candidate = Candidate {
        source_id: bfs::metadata::digest(url.as_bytes()),
        url: url.clone(),
        title: "Test".into(),
        name: "a.zip".into(),
        discovered_at: "2026-10-31T00:00:00Z".into(),
    };
    let config = DownloadConfig {
        staging_dir: root.path().join("downloads"),
        max_file_size: limit.into(),
        max_speed: speed,
        timeout_seconds: 1,
        ..Default::default()
    };
    let report = download::run(
        &config,
        &SourceConfig {
            page: url,
            ..Default::default()
        },
        &Inventory {
            version: 1,
            candidates: vec![candidate],
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    thread.join().unwrap();
    (root, config, report)
}
#[test]
fn downloaded_payload_is_sealed_and_receipted() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, config, report) = transfer(b"fixture", "1 KB");
    assert_eq!(report.downloaded, 1, "{:?}", report.errors);
    let path = config
        .staging_dir
        .join(download::payload_path(&config, &report.candidates[0]));
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0
    );
}
#[test]
fn over_limit_payload_is_not_published() {
    let (_root, config, report) = transfer(b"too large", "3 B");
    assert_eq!(report.failed, 1);
    assert!(
        !config
            .staging_dir
            .join(download::payload_path(&config, &report.candidates[0]))
            .exists()
    );
}

#[test]
fn sealed_receipt_requires_explicit_unlock_then_verifies_hash() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, config, report) = transfer(b"fixture", "1 KB");
    assert!(
        bfs::acquisition::completed_inputs(&config)
            .unwrap_err()
            .to_string()
            .contains("unlock")
    );
    let path = config
        .staging_dir
        .join(download::payload_path(&config, &report.candidates[0]));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        bfs::acquisition::completed_inputs(&config).unwrap().len(),
        1
    );
    std::fs::write(&path, b"changed").unwrap();
    assert!(bfs::acquisition::completed_inputs(&config).is_err());
}

#[test]
fn downloaded_zip_flows_through_existing_ingest() {
    use std::{io::Write, os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};
    let mut bytes = std::io::Cursor::new(vec![]);
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"hello world").unwrap();
        zip.finish().unwrap();
    }
    let (root, config, report) = transfer(Box::leak(bytes.into_inner().into_boxed_slice()), "1 MB");
    let payload = config
        .staging_dir
        .join(download::payload_path(&config, &report.candidates[0]));
    std::fs::set_permissions(payload, std::fs::Permissions::from_mode(0o600)).unwrap();
    let config_path = root.path().join("collection.toml");
    std::fs::write(
        &config_path,
        format!(
            "output = '{}'\nzip = true\n[download]\nstaging_dir = '{}'\n",
            root.path().join("sorted.zip").display(),
            config.staging_dir.display()
        ),
    )
    .unwrap();
    use clap::Parser;
    let bfs::cli::Command::Ingest(args) =
        bfs::cli::Cli::parse_from(["bfs", "ingest", "--config", config_path.to_str().unwrap()])
            .command
    else {
        panic!()
    };
    let ingest = bfs::config::Config::for_ingest(&args).unwrap();
    let preview = bfs::pipeline::preview(&ingest, &AtomicBool::new(false)).unwrap();
    assert_eq!(preview.plan.len(), 1);
    let result = bfs::pipeline::ingest(&ingest, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.accepted, 1);
    assert_eq!(result.failures, 0);
    assert!(root.path().join("sorted.zip").is_file());
}

#[test]
fn unicode_titles_fit_filesystem_component_limits() {
    let config = DownloadConfig::default();
    let candidate = Candidate {
        source_id: "a".repeat(64),
        url: "http://localhost/a.zip".into(),
        title: "界".repeat(100),
        name: format!("{}.zip", "界".repeat(100)),
        discovered_at: String::new(),
    };
    let path = download::payload_path(&config, &candidate);
    assert!(path.components().all(|c| c.as_os_str().len() <= 255));
}

#[test]
fn final_chunk_pacing_cannot_publish_after_timeout() {
    let (_root, config, report) = transfer_with_speed(b"fixture", "1 KB", Some("1 B/s".into()));
    assert_eq!(report.failed, 1);
    assert!(
        !config
            .staging_dir
            .join(download::payload_path(&config, &report.candidates[0]))
            .exists()
    );
}

#[test]
fn changed_link_title_reuses_completed_source() {
    use std::sync::atomic::AtomicBool;
    let (_root, config, report) = transfer(b"fixture", "1 KB");
    let mut candidate = report.candidates[0].clone();
    candidate.title = "New title".into();
    let source = SourceConfig {
        page: candidate.url.clone(),
        ..Default::default()
    };
    let result = download::run(
        &config,
        &source,
        &Inventory {
            version: 1,
            candidates: vec![candidate],
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.cached, 1);
    assert_eq!(result.downloaded, 0);
}
