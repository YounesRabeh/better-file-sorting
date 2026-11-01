use crate::common;
use bfs::organization::rules::{Preset, Rules};
#[test]
fn embedded_presets_and_portable_exports_preserve_provenance() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("mixed.zip");
    let root = t.path().join("repo");
    common::archive(
        &input,
        &[
            ("a.PDF", b"%PDF-1.7\n"),
            ("same.pdf", b"%PDF-1.7\n"),
            ("other.bin", b"%PDF-1.7\n"),
        ],
    );
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
    let records = bfs::repository::metadata::load(&root).unwrap();
    for (preset, expected) in [
        (Preset::ByFormat, "pdf"),
        (Preset::ByExtension, "bin"),
        (Preset::ByMismatch, "compatible"),
    ] {
        let rules = Rules::for_preset(preset);
        let plan = rules.plan(&records).unwrap();
        assert_eq!(plan.len(), 3);
        assert!(
            plan.iter()
                .any(|r| r.path.starts_with(&format!("{expected}/")))
        );
    }
    let output = t.path().join("out.zip");
    let result = common::run(&[
        "export".as_ref(),
        root.as_os_str(),
        "--preset".as_ref(),
        "by-format".as_ref(),
        "-o".as_ref(),
        output.as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let header: serde_json::Value =
        serde_json::from_reader(zip.by_name("export.json").unwrap()).unwrap();
    assert_eq!(header["schema_version"], 2);
    use std::io::Read;
    let mut text = String::new();
    zip.by_name("manifest.jsonl")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let rows: Vec<serde_json::Value> = text
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .any(|r| r["occurrence"]["original_name"] == "a.PDF")
    );
    assert!(
        rows.iter()
            .all(|r| r["occurrence"]["detection"]["detected_format"] == "pdf")
    );
}

#[test]
fn resume_export_skips_verified_archives_and_rejects_tampering_or_changed_plan() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("a.zip");
    let root = t.path().join("repo");
    let out = t.path().join("exports");
    common::archive(&input, &[("a.pdf", b"%PDF-1.7\n"), ("b.txt", b"hello")]);
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
    let args = [
        "export".as_ref(),
        root.as_os_str(),
        "--preset".as_ref(),
        "by-format".as_ref(),
        "--zip-each-dir".as_ref(),
        "-o".as_ref(),
        out.as_os_str(),
        "--resume".as_ref(),
    ];
    let first = common::run(&args);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let pdf = out.join("pdf.zip");
    let before = std::fs::metadata(&pdf).unwrap().modified().unwrap();
    assert!(common::run(&args).status.success());
    assert_eq!(before, std::fs::metadata(&pdf).unwrap().modified().unwrap());
    std::fs::remove_file(out.join("text.zip")).unwrap();
    let missing = common::run(&args);
    assert!(
        missing.status.success(),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    assert_eq!(before, std::fs::metadata(&pdf).unwrap().modified().unwrap());
    let checkpoint = std::fs::read_dir(root.join("operations/export"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&checkpoint).unwrap()).unwrap();
    state["archives"]
        .as_object_mut()
        .unwrap()
        .remove("text.zip");
    state["completed"] = false.into();
    std::fs::remove_file(out.join("text.zip")).unwrap();
    std::fs::write(&checkpoint, serde_json::to_vec(&state).unwrap()).unwrap();
    let retry = common::run(&args);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert!(out.join("text.zip").exists());
    assert_eq!(before, std::fs::metadata(&pdf).unwrap().modified().unwrap());
    let changed = common::run(&[
        "export".as_ref(),
        root.as_os_str(),
        "--preset".as_ref(),
        "by-extension".as_ref(),
        "--zip-each-dir".as_ref(),
        "-o".as_ref(),
        out.as_os_str(),
        "--resume".as_ref(),
    ]);
    assert!(!changed.status.success());
    std::fs::write(&pdf, b"tampered").unwrap();
    let tampered = common::run(&args);
    assert!(!tampered.status.success());
    assert_eq!(std::fs::read(pdf).unwrap(), b"tampered");
}

#[test]
fn crash_recovery_rejects_corrupted_layout_metadata() {
    use std::io::{Read, Write};
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("a.zip");
    let root = t.path().join("repo");
    let out = t.path().join("out");
    common::archive(&input, &[("a.pdf", b"%PDF-1.7\n")]);
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
    let args = [
        "export".as_ref(),
        root.as_os_str(),
        "--zip-each-dir".as_ref(),
        "-o".as_ref(),
        out.as_os_str(),
        "--resume".as_ref(),
    ];
    assert!(common::run(&args).status.success());
    let state_path = std::fs::read_dir(root.join("operations/export"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    state["archives"].as_object_mut().unwrap().clear();
    state["completed"] = false.into();
    std::fs::write(state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    let path = out.join("pdf.zip");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut entries = vec![];
    for i in 0..archive.len() {
        let mut e = archive.by_index(i).unwrap();
        let name = e.name().unwrap().into_owned();
        let mut b = vec![];
        e.read_to_end(&mut b).unwrap();
        entries.push((name, b));
    }
    drop(archive);
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes) in entries {
        writer
            .start_file(&name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(if name == "layout.jsonl" {
                b"{\"sha256\":\"bogus\"}\n"
            } else {
                &bytes
            })
            .unwrap();
    }
    writer.finish().unwrap();
    let result = common::run(&args);
    assert!(!result.status.success(), "corrupted layout was accepted");
}

#[test]
fn automatic_fallback_exports_each_dynamic_group_and_keeps_explicit_rules() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("mixed.zip");
    common::archive(
        &input,
        &[
            ("fake.foo", b"%PDF-1.7\n"),
            ("a.NEWTYPE", b"\0\xff\x01"),
            ("b.newtype", b"\0\xff\x01"),
            ("unknown", b"\0\xfe"),
            ("weak.xyz", b"ordinary prose"),
            ("conflict.bin", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR %PDF-1.7\n"),
        ],
    );
    let root = t.path().join("repo");
    let ingest = common::run(&[
        "ingest".as_ref(),
        input.as_os_str(),
        "-o".as_ref(),
        root.as_os_str(),
        "--security-mode".as_ref(),
        "standard".as_ref(),
        "--preset".as_ref(),
        "by-format-or-extension".as_ref(),
    ]);
    assert!(
        ingest.status.success(),
        "{}",
        String::from_utf8_lossy(&ingest.stderr)
    );
    let output = t.path().join("automatic");
    let args = [
        "export".as_ref(),
        root.as_os_str(),
        "--preset".as_ref(),
        "by-format-or-extension".as_ref(),
        "--zip-each-dir".as_ref(),
        "--output".as_ref(),
        output.as_os_str(),
        "--resume".as_ref(),
    ];
    let result = common::run(&args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (group, count) in [
        ("detected/pdf", 1),
        ("undetected/extension-newtype", 2),
        ("undetected/no-extension", 1),
        ("undetected/extension-xyz", 1),
        ("ambiguous", 1),
    ] {
        let mut zip =
            zip::ZipArchive::new(std::fs::File::open(output.join(format!("{group}.zip"))).unwrap())
                .unwrap();
        let mut manifest = String::new();
        use std::io::Read;
        zip.by_name("manifest.jsonl")
            .unwrap()
            .read_to_string(&mut manifest)
            .unwrap();
        assert_eq!(manifest.lines().count(), count);
    }
    assert!(common::run(&args).status.success());
    let rows = bfs::repository::metadata::load(&root).unwrap();
    let explicit = Rules::parse("[organization]\nduplicates = 'all'\ndefault_directory = 'remainder'\n[[rules]]\nname = 'selected-extension'\n[rules.match]\noriginal_extension = 'newtype'\n[rules.output]\ndirectory = 'my-selection'\nfilename = '{sha256}.sample'\n").unwrap();
    let plan = explicit.plan(&rows).unwrap();
    assert_eq!(
        plan.iter()
            .filter(|p| p.path.starts_with("my-selection/"))
            .count(),
        2
    );
    assert_eq!(plan.len(), 6);
    let both = t.path().join("both.toml");
    std::fs::write(&both, "[organization]\nduplicates = 'all'\ndefault_directory = '{format_or_extension}'\n[[rules]]\nname = 'selected-extension'\n[rules.match]\noriginal_extension = 'newtype'\n[rules.output]\ndirectory = 'my-selection'\nfilename = '{sha256}.sample'\n").unwrap();
    let custom = t.path().join("custom");
    let out = common::run(&[
        "export".as_ref(),
        root.as_os_str(),
        "--rules".as_ref(),
        both.as_os_str(),
        "--zip-each-dir".as_ref(),
        "--output".as_ref(),
        custom.as_os_str(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(custom.join("my-selection.zip").is_file());
    assert!(custom.join("detected/pdf.zip").is_file());
    assert!(custom.join("ambiguous.zip").is_file());
    let direct = t.path().join("direct");
    let out = common::run(&[
        "ingest".as_ref(),
        input.as_os_str(),
        "--preset".as_ref(),
        "by-format-or-extension".as_ref(),
        "--zip-each-dir".as_ref(),
        "--output".as_ref(),
        direct.as_os_str(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(direct.join("detected/pdf.zip").is_file());
}
