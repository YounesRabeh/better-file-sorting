// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Integration checks for extraction tests.
//! Exercises public BFS behavior with disposable fixtures; failures should identify
//! contract regressions without changing user repositories.

use bfs::{config::Limits, extraction::extract};
use std::{fs::File, io::Write, sync::atomic::AtomicBool};
use zip::{ZipWriter, write::SimpleFileOptions};
/// Build an encrypted or plain single-entry ZIP fixture.
fn zip(name: &str, bytes: &[u8], encrypted: bool) -> (tempfile::TempDir, zip::ZipArchive<File>) {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("test.zip");
    let mut w = ZipWriter::new(File::create(&path).unwrap());
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let options = if encrypted {
        options.with_aes_encryption(zip::AesMode::Aes256, "fixture-password")
    } else {
        options
    };
    w.start_file(name, options).unwrap();
    w.write_all(bytes).unwrap();
    w.finish().unwrap();
    let a = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    (d, a)
}
#[test]
/// Ensure extraction hashes and counts streamed content.
fn stream_hashes_harmless_content() {
    let (d, mut a) = zip("safe.txt", b"abc", false);
    let mut total = 0;
    let item = extract(
        &mut a,
        0,
        &[],
        d.path(),
        &Limits::default(),
        &mut total,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        item.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(item.size, 3);
    assert_eq!(item.prefix, b"abc");
    assert_eq!(total, 3);
}
#[test]
/// Ensure AES extraction retries passwords and rejects wrong candidates.
fn aes_retries_candidates_and_checks_integrity() {
    let (d, mut a) = zip("safe.txt", b"abc", true);
    let mut total = 0;
    assert!(
        extract(
            &mut a,
            0,
            &["wrong".into()],
            d.path(),
            &Limits::default(),
            &mut total,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    let item = extract(
        &mut a,
        0,
        &["wrong".into(), "fixture-password".into()],
        d.path(),
        &Limits::default(),
        &mut total,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(item.size, 3);
}
#[test]
/// Ensure hostile names, limits, and cancellation clean up staging.
fn hostile_names_limits_and_interruptions_leave_no_staging_files() {
    for name in ["../escape", "/absolute", "C:\\escape", "a\\..\\escape"] {
        let (d, mut a) = zip(name, b"abc", false);
        assert!(
            extract(
                &mut a,
                0,
                &[],
                d.path(),
                &Limits::default(),
                &mut 0,
                &AtomicBool::new(false)
            )
            .is_err(),
            "{name}"
        );
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }
    let (d, mut a) = zip("safe", b"abc", false);
    let limits = Limits {
        max_entry_bytes: 2,
        ..Limits::default()
    };
    assert!(
        extract(
            &mut a,
            0,
            &[],
            d.path(),
            &limits,
            &mut 0,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(
        extract(
            &mut a,
            0,
            &[],
            d.path(),
            &Limits::default(),
            &mut 0,
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
/// Ensure staged payload paths are unreadable before publication.
fn staging_path_is_inaccessible_even_before_commit() {
    use std::os::unix::fs::PermissionsExt;
    let (d, mut a) = zip("safe.txt", b"abc", false);
    let item = extract(
        &mut a,
        0,
        &[],
        d.path(),
        &Limits::default(),
        &mut 0,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        item.file.as_file().metadata().unwrap().permissions().mode() & 0o777,
        0
    );
    assert!(std::fs::read(item.file.path()).is_err());
}
