// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Younidev. See LICENSE for license terms.

//! Untrusted inputs harness for BFS.
//! Uses synthetic inputs to exercise the library independently of normal CLI usage.
//! Keep harness work isolated from user payloads and production repositories.

#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return;
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = bfs::organization::rules::Rules::parse(text);
        let _ = bfs::ingest::extraction::validate_name(text);
        let _ = bfs::organization::rules::sanitize(text);
    }
    let _ = bfs::detection::classifier::classify(data);
    let detection = bfs::detection::classifier::detect(data);
    let _ = bfs::detection::classifier::detect_reader(data, &mut std::io::Cursor::new(data));
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = bfs::detection::mismatch::extension(text);
        let _ = bfs::detection::mismatch::normalize_extension(text);
        let _ = bfs::detection::mismatch::evaluate(text, &detection);
        if let Ok(record) = serde_json::from_str::<bfs::repository::metadata::Occurrence>(text) {
            if record.validate().is_ok() {
                let records = [record];
                let report = bfs::output::report::mismatches(
                    &records,
                    &[],
                    bfs::detection::classifier::Confidence::Unknown,
                    true,
                );
                for format in [
                    bfs::output::report::ReportFormat::Json,
                    bfs::output::report::ReportFormat::Jsonl,
                    bfs::output::report::ReportFormat::Csv,
                    bfs::output::report::ReportFormat::Table,
                ] {
                    let _ = report.write(std::io::sink(), format);
                }
            }
        }
    }
    if let Ok(archive) = zip::ZipArchive::new(std::io::Cursor::new(data)) {
        for index in 0..archive.len().min(1000) {
            if let Ok(entry) = archive.by_index_data(index) {
                let _ = entry.name();
            }
        }
    }
});
