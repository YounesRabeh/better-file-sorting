mod common;
use bfs::classifier;

fn pe() -> Vec<u8> {
    let mut b = vec![0; 512];
    b[..2].copy_from_slice(b"MZ");
    b[60..64].copy_from_slice(&128u32.to_le_bytes());
    b[128..132].copy_from_slice(b"PE\0\0");
    b[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
    b[134..136].copy_from_slice(&1u16.to_le_bytes());
    b[148..150].copy_from_slice(&240u16.to_le_bytes());
    b[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
    b
}
fn elf() -> Vec<u8> {
    let mut b = vec![0; 64];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    b[16] = 2;
    b[18] = 62;
    b[20] = 1;
    b[52] = 64;
    b
}
#[test]
fn structured_detection_requires_headers_and_never_uses_extensions() {
    for (b, format) in [
        (pe(), "pe"),
        (elf(), "elf"),
        (b"junk\n%PDF-1.7\n".to_vec(), "pdf"),
    ] {
        let d = classifier::detect(&b);
        assert_eq!(d.detected_format.as_str(), format);
        assert_eq!(d.confidence, classifier::Confidence::High);
    }
    for bytes in [b"MZ".as_slice(), b"\x7fELF", b"PK\x03\x04"] {
        assert_ne!(
            classifier::detect(bytes).confidence,
            classifier::Confidence::High
        );
    }
}
#[test]
fn signatures_text_and_truncation() {
    let fixtures: &[(&[u8], &str)] = &[
        (b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", "png"),
        (b"\xff\xd8\xff\xe0\0\x10JFIF\0", "jpeg"),
        (b"GIF89a\x01\0\x01\0\0\0\0", "gif"),
        (b"{\\rtf1 hello}", "rtf"),
        (b"SQLite format 3\0", "sqlite"),
        (b"\0asm\x01\0\0\0", "wasm"),
        (b"#!/usr/bin/env python3\nprint('hello')\n", "python"),
        (b"#!/bin/sh\necho hello\n", "shell"),
        (b"#!/usr/bin/env node\nconsole.log('hello')\n", "javascript"),
        (b"#!/usr/bin/pwsh\nWrite-Output hello\n", "powershell"),
        (b"@echo off\r\necho hello\r\n", "batch"),
        (b"<!DOCTYPE html><html></html>", "html"),
        (b"<?xml version=\"1.0\"?><root/>", "xml"),
        (b"{\"hello\": [1, 2]}", "json"),
        (b"ordinary prose", "text"),
    ];
    for (b, expected) in fixtures {
        assert_eq!(
            classifier::detect(b).detected_format.as_str(),
            *expected,
            "{b:?}"
        );
        for end in 0..b.len() {
            let _ = classifier::detect(&b[..end]);
        }
    }
}
#[test]
fn zip_markers_are_structural_and_conflicts_are_ambiguous() {
    let t = tempfile::tempdir().unwrap();
    for (names, expected) in [
        (vec!["[Content_Types].xml", "word/document.xml"], "docx"),
        (vec!["[Content_Types].xml", "xl/workbook.xml"], "xlsx"),
        (vec!["[Content_Types].xml", "ppt/presentation.xml"], "pptx"),
        (vec!["META-INF/MANIFEST.MF"], "jar"),
        (vec!["AndroidManifest.xml", "classes.dex"], "apk"),
        (vec!["word/document.xml"], "zip"),
        (
            vec![
                "[Content_Types].xml",
                "word/document.xml",
                "xl/workbook.xml",
            ],
            "ambiguous",
        ),
        (vec!["data.txt"], "zip"),
    ] {
        let path = t.path().join("fixture.zip");
        let entries: Vec<_> = names.iter().map(|n| (*n, b"benign".as_slice())).collect();
        common::archive(&path, &entries);
        let b = std::fs::read(path).unwrap();
        let detection = classifier::detect(&b);
        assert_eq!(detection.detected_format.as_str(), expected);
        if expected != "ambiguous" {
            let filename = format!("fixture.{expected}");
            assert_eq!(
                bfs::mismatch::evaluate(&filename, &detection).status,
                bfs::mismatch::Status::Match
            );
        }
    }
}
#[test]
fn extension_compatibility_is_conservative() {
    use bfs::mismatch::{Status, evaluate};
    let pe = classifier::detect(&pe());
    let elf = classifier::detect(&elf());
    let text = classifier::detect(b"ordinary text");
    let unknown = classifier::detect(b"\0\xff");
    let jpeg = classifier::detect(b"\xff\xd8\xff\xe0\0\x10JFIF\0");
    assert_eq!(evaluate("a.exe", &unknown).status, Status::UnknownFormat);
    assert_eq!(evaluate("a.JPG", &jpeg).status, Status::Match);
    assert_eq!(evaluate("a.jpeg", &jpeg).status, Status::Match);
    for (name, d, status) in [
        ("a.exe", &pe, Status::Match),
        ("a.DLL", &pe, Status::Match),
        ("invoice.exe.pdf", &pe, Status::Mismatch),
        ("a.exe", &elf, Status::Mismatch),
        ("sample", &elf, Status::MissingExtension),
        ("sample.bin", &pe, Status::Compatible),
        ("sample.dat", &elf, Status::Compatible),
        ("a.js", &text, Status::Ambiguous),
        ("a.unregistered", &pe, Status::UnknownExtension),
    ] {
        assert_eq!(evaluate(name, d).status, status, "{name}");
    }
}

#[test]
fn ingestion_reports_stats_rules_and_independent_exports() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("input.zip");
    let repo = t.path().join("repo");
    let pe = pe();
    let elf = elf();
    common::archive(
        &input,
        &[
            ("invoice.exe.pdf", &pe),
            ("good.EXE", &pe),
            ("loader.exe", &elf),
            ("sample", &elf),
            ("note.js", b"hello"),
        ],
    );
    let result = common::run(&[
        "ingest".as_ref(),
        input.as_os_str(),
        "--output".as_ref(),
        repo.as_os_str(),
        "--security-mode".as_ref(),
        "standard".as_ref(),
        "--workers".as_ref(),
        "4".as_ref(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let records = common::manifest(&repo);
    assert_eq!(records.len(), 5);
    assert!(
        records
            .iter()
            .all(|r| r["detection"]["detector_version"] == "bfs-1")
    );
    let report = common::run(&[
        "report".as_ref(),
        "mismatches".as_ref(),
        repo.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["rows"].as_array().unwrap().len(), 2);
    assert_eq!(report["counts"]["mismatch"], 2);
    let stats = common::run(&["stats".as_ref(), repo.as_os_str()]);
    let stats: serde_json::Value = serde_json::from_slice(&stats.stdout).unwrap();
    assert_eq!(stats["occurrences"], 5);
    assert_eq!(stats["unique_samples"], 3);
    assert_eq!(stats["by_detected_format"]["pe"]["occurrences"], 2);
    assert_eq!(stats["by_detected_format"]["pe"]["unique_hashes"], 1);
    std::fs::remove_file(input).unwrap();
    let before = common::manifest(&repo);
    for (field, folder) in [
        ("detected_format", "formats"),
        ("original_extension", "extensions"),
    ] {
        let rules = t.path().join(format!("{folder}.toml"));
        std::fs::write(
            &rules,
            format!("[organization]\ndefault_directory = '{{{field}}}'\n"),
        )
        .unwrap();
        let out = t.path().join(folder);
        let export = common::run(&[
            "export".as_ref(),
            repo.as_os_str(),
            "--rules".as_ref(),
            rules.as_os_str(),
            "--zip-each-dir".as_ref(),
            "--output".as_ref(),
            out.as_os_str(),
        ]);
        assert!(
            export.status.success(),
            "{}",
            String::from_utf8_lossy(&export.stderr)
        );
        assert!(
            out.join(if folder == "formats" {
                "pe.zip"
            } else {
                "pdf.zip"
            })
            .exists()
        );
    }
    assert_eq!(before, common::manifest(&repo));
}

#[test]
fn legacy_defaults_and_explicit_analysis() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("input.zip");
    let repo = t.path().join("repo");
    common::archive(&input, &[("invoice.pdf", &pe())]);
    assert!(
        common::run(&[
            "ingest".as_ref(),
            input.as_os_str(),
            "-o".as_ref(),
            repo.as_os_str(),
            "--security-mode".as_ref(),
            "standard".as_ref()
        ])
        .status
        .success()
    );
    for entry in std::fs::read_dir(repo.join("manifests")).unwrap() {
        let p = entry.unwrap().path();
        let text = std::fs::read_to_string(&p).unwrap();
        let mut r: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        r.as_object_mut().unwrap().remove("detection");
        r.as_object_mut().unwrap().remove("mismatch");
        std::fs::write(p, format!("{r}\n")).unwrap();
    }
    let records = bfs::metadata::load(&repo).unwrap();
    assert_eq!(
        records[0].detection.detected_format,
        classifier::Format::NotEvaluated
    );
    assert_eq!(
        records[0].mismatch.status,
        bfs::mismatch::Status::NotEvaluated
    );
    let result = common::run(&["analyze".as_ref(), repo.as_os_str()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        bfs::metadata::load(&repo).unwrap()[0].mismatch.status,
        bfs::mismatch::Status::Mismatch
    );
}

#[test]
fn remaining_signatures_and_random_inputs_are_bounded() {
    let fixtures: &[(&[u8], usize, &str)] = &[
        (b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", 512, "cfb"),
        (b"Rar!\x1a\x07\0", 14, "rar"),
        (b"Rar!\x1a\x07\x01\0", 16, "rar"),
        (b"7z\xbc\xaf\x27\x1c", 32, "7z"),
        (b"\x1f\x8b\x08\0", 10, "gzip"),
        (b"BZh9", 10, "bzip2"),
        (b"\xfd7zXZ\0", 12, "xz"),
        (b"BM", 54, "bmp"),
        (b"II\x2a\0", 8, "tiff"),
        (b"MM\0\x2a", 8, "tiff"),
        (b"\xfe\xed\xfa\xce", 28, "macho"),
        (b"\xcf\xfa\xed\xfe", 32, "macho"),
        (b"RIFF\x04\0\0\0WEBP", 12, "webp"),
    ];
    for (magic, len, format) in fixtures {
        let mut bytes = vec![0; *len];
        bytes[..magic.len()].copy_from_slice(magic);
        if *format == "bmp" {
            bytes[2..6].copy_from_slice(&54u32.to_le_bytes());
            bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
            bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
            bytes[18..22].copy_from_slice(&1u32.to_le_bytes());
            bytes[22..26].copy_from_slice(&1u32.to_le_bytes());
            bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
            bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
        }
        if *format == "macho" {
            if bytes[0] == 0xfe {
                bytes[4..8].copy_from_slice(&7u32.to_be_bytes());
                bytes[12..16].copy_from_slice(&2u32.to_be_bytes());
            } else {
                bytes[4..8].copy_from_slice(&7u32.to_le_bytes());
                bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
            }
        }
        assert_eq!(classifier::detect(&bytes).detected_format.as_str(), *format);
        for end in 0..bytes.len() {
            let _ = classifier::detect(&bytes[..end]);
        }
    }
    let mut tar = vec![0; 512];
    tar[257..263].copy_from_slice(b"ustar\0");
    tar[148..156].fill(b' ');
    let sum: u64 = tar.iter().map(|b| *b as u64).sum();
    tar[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    assert_eq!(
        classifier::detect(&tar).detected_format,
        classifier::Format::Tar
    );
    tar[0] ^= 1;
    assert_ne!(
        classifier::detect(&tar).detected_format,
        classifier::Format::Tar
    );
    let mut polyglot = pe();
    polyglot[400..409].copy_from_slice(b"\n%PDF-1.7");
    assert_eq!(
        classifier::detect(&polyglot).detected_format,
        classifier::Format::Ambiguous
    );
    let mut invalid = pe();
    invalid[60..64].fill(255);
    assert_ne!(
        classifier::detect(&invalid).detected_format,
        classifier::Format::Pe
    );
    let mut state = 17u64;
    for n in 0..512 {
        let mut b = vec![0; n * 17];
        for byte in &mut b {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        let detection = classifier::detect(&b);
        let mut cursor = std::io::Cursor::new(&b);
        let _ = classifier::detect_reader(&b, &mut cursor);
        let name = String::from_utf8_lossy(&b);
        let _ = bfs::mismatch::evaluate(&name, &detection);
    }
}

#[test]
fn large_zip_uses_bounded_directory_reads_and_generic_zip_is_uncertain() {
    use std::io::{Read, Seek, SeekFrom};
    struct Meter {
        inner: std::io::Cursor<Vec<u8>>,
        bytes: usize,
    }
    impl Read for Meter {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(b)?;
            self.bytes += n;
            Ok(n)
        }
    }
    impl Seek for Meter {
        fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(p)
        }
    }
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("large.zip");
    let mut state = 93u64;
    let padding: Vec<u8> = (0..2 * 1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    common::archive(
        &path,
        &[
            ("padding", &padding),
            ("[Content_Types].xml", b"types"),
            ("word/document.xml", b"document"),
        ],
    );
    let bytes = std::fs::read(path).unwrap();
    let prefix = bytes[..8192].to_vec();
    let mut meter = Meter {
        inner: std::io::Cursor::new(bytes),
        bytes: 0,
    };
    let detection = classifier::detect_reader(&prefix, &mut meter);
    assert_eq!(detection.detected_format, classifier::Format::Docx);
    assert!(meter.bytes < 70000, "{}", meter.bytes);
    assert_eq!(
        bfs::mismatch::evaluate("a.docx", &detection).status,
        bfs::mismatch::Status::Match
    );
    assert_eq!(
        bfs::mismatch::evaluate("a.zip", &detection).status,
        bfs::mismatch::Status::Compatible
    );
    let mut generic = classifier::detect(b"PK\x03\x04");
    generic.confidence = classifier::Confidence::High;
    assert_eq!(
        bfs::mismatch::evaluate("a.docx", &generic).status,
        bfs::mismatch::Status::Ambiguous
    );
    assert_eq!(bfs::mismatch::extension("a.PDF.EXE"), "exe");
    assert_eq!(bfs::mismatch::extension("a.TAR.GZ"), "tar.gz");
    assert_eq!(bfs::mismatch::normalize_extension("..JPEG"), "jpeg");
}

#[test]
#[cfg(unix)]
fn encrypted_ingestion_sealed_reports_and_export_refusal() {
    use std::{fs::File, io::Write, os::unix::fs::PermissionsExt};
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("encrypted.zip");
    let repo = t.path().join("repo");
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    zip.start_file(
        "invoice.pdf",
        zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes256, "fixture"),
    )
    .unwrap();
    zip.write_all(&pe()).unwrap();
    zip.finish().unwrap();
    let password = t.path().join("password.txt");
    std::fs::write(&password, "fixture\n").unwrap();
    std::fs::set_permissions(&password, std::fs::Permissions::from_mode(0o600)).unwrap();
    let out = common::run(&[
        "ingest".as_ref(),
        path.as_os_str(),
        "-o".as_ref(),
        repo.as_os_str(),
        "--password-file".as_ref(),
        password.as_os_str(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let hash = common::manifest(&repo)[0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let object = repo.join("objects").join(&hash[..2]).join(&hash);
    for format in ["table", "json", "jsonl", "csv"] {
        let out = common::run(&[
            "report".as_ref(),
            "mismatches".as_ref(),
            repo.as_os_str(),
            "--format".as_ref(),
            format.as_ref(),
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("invoice.pdf"));
    }
    let out = t.path().join("out.zip");
    assert!(
        common::run(&[
            "export".as_ref(),
            repo.as_os_str(),
            "-o".as_ref(),
            out.as_os_str(),
            "--dry-run".as_ref()
        ])
        .status
        .success()
    );
    assert!(!out.exists());
    assert!(
        !common::run(&[
            "export".as_ref(),
            repo.as_os_str(),
            "-o".as_ref(),
            out.as_os_str()
        ])
        .status
        .success()
    );
    assert!(
        !common::run(&["analyze".as_ref(), repo.as_os_str()])
            .status
            .success()
    );
    assert_eq!(
        std::fs::metadata(object).unwrap().permissions().mode() & 0o777,
        0
    );
}

#[test]
fn report_escaping_filtering_and_new_rule_matchers() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("a.zip");
    let repo = t.path().join("repo");
    common::archive(&input, &[("comma,quote\".pdf", &pe()), ("good.exe", &pe())]);
    assert!(common::ingest(&input, &repo, "2").status.success());
    let records = bfs::metadata::load(&repo).unwrap();
    let report = bfs::report::mismatches(&records, &[], classifier::Confidence::High, false);
    let mut csv = vec![];
    report
        .write(&mut csv, bfs::report::ReportFormat::Csv)
        .unwrap();
    assert!(
        String::from_utf8(csv)
            .unwrap()
            .contains("\"comma,quote\"\".pdf\"")
    );
    let report = bfs::report::mismatches(
        &records,
        &[bfs::mismatch::Status::Match],
        classifier::Confidence::High,
        false,
    );
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].original_entry_name, "good.exe");
    let rules = bfs::rules::Rules::parse("[[rules]]\nname='mismatch'\n[rules.match]\ndetected_format='pe'\nmismatch_status='mismatch'\n[rules.output]\ndirectory='{detected_family}/{detected_format}/{mismatch_status}'\nfilename='{sha256}.sample'\n[organization]\nduplicates='all'\n").unwrap();
    assert!(
        rules
            .plan(&records)
            .unwrap()
            .iter()
            .any(|p| p.path.starts_with("executable/pe/mismatch/"))
    );
    let out = t.path().join("report.csv");
    let args = [
        "report".as_ref(),
        "mismatches".as_ref(),
        repo.as_os_str(),
        "--format".as_ref(),
        "csv".as_ref(),
        "-o".as_ref(),
        out.as_os_str(),
    ];
    assert!(common::run(&args).status.success());
    let before = std::fs::read(&out).unwrap();
    assert!(!common::run(&args).status.success());
    assert_eq!(before, std::fs::read(out).unwrap());
}

#[test]
fn export_trusts_persisted_metadata_and_never_rehashes_or_reclassifies() {
    use std::io::Read;
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("a.zip");
    let root = t.path().join("repo");
    common::archive(&input, &[("a.pdf", b"%PDF-1.7\nfixture\n")]);
    assert!(
        common::run(&[
            "ingest".as_ref(),
            input.as_os_str(),
            "-o".as_ref(),
            root.as_os_str(),
            "--security-mode".as_ref(),
            "standard".as_ref()
        ])
        .status
        .success()
    );
    std::fs::remove_file(input).unwrap();
    let repo = bfs::storage::Repository::existing(&root).unwrap();
    let records = bfs::metadata::load(&root).unwrap();
    let pointer = std::fs::read(root.join("views/CURRENT")).unwrap();
    let manifest = common::manifest(&root);
    // Deliberately violate storage integrity to prove export uses neither hashing nor detection.
    let bytes = vec![0xff; records[0].size as usize];
    std::fs::write(repo.object(&records[0].sha256).unwrap(), &bytes).unwrap();
    let config = bfs::config::Config {
        zip: true,
        output: t.path().join("out.zip"),
        ..Default::default()
    };
    let rules = bfs::rules::Rules::parse("[organization]\ndefault_directory='{detected_format}'\n")
        .unwrap();
    bfs::archive::export_repository(
        &repo,
        &rules,
        &config,
        false,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&config.output).unwrap()).unwrap();
    let mut exported = vec![];
    zip.by_name(&format!("pdf/{}.sample", records[0].sha256))
        .unwrap()
        .read_to_end(&mut exported)
        .unwrap();
    assert_eq!(exported, bytes);
    assert_eq!(pointer, std::fs::read(root.join("views/CURRENT")).unwrap());
    assert_eq!(manifest, common::manifest(&root));
}

#[test]
fn prose_and_indented_examples_are_not_definitive_mismatches() {
    for (name, bytes) in [
        (
            "cars.txt",
            b"BMW manufactures vehicles. This is an ordinary plain text document.\n".as_slice(),
        ),
        ("README.md", b"    #!/bin/sh\n    echo example\n".as_slice()),
    ] {
        let d = classifier::detect(bytes);
        assert_eq!(d.detected_format, classifier::Format::Text);
        assert_ne!(
            bfs::mismatch::evaluate(name, &d).status,
            bfs::mismatch::Status::Mismatch
        );
    }
}
#[test]
fn malformed_executable_headers_never_get_high_confidence() {
    let mut invalid_elf = elf();
    invalid_elf[16..18].fill(0);
    invalid_elf[52..54].fill(0);
    let mut invalid_macho = vec![0; 28];
    invalid_macho[..4].copy_from_slice(b"\xcf\xfa\xed\xfe");
    for bytes in [invalid_elf, invalid_macho] {
        assert_ne!(
            classifier::detect(&bytes).confidence,
            classifier::Confidence::High
        );
    }
}
#[test]
fn container_and_image_pdf_conflicts_are_ambiguous() {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    zip.start_file(
        "note.txt",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(b"Example format header:\n%PDF-1.7\n")
        .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert_eq!(
        classifier::detect(&bytes).detected_format,
        classifier::Format::Ambiguous
    );
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\n%PDF-1.7\n";
    assert_eq!(
        classifier::detect(png).detected_format,
        classifier::Format::Ambiguous
    );
}
