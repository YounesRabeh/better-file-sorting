//! End-to-end checks of catalog authority, interchange and legacy compatibility.
#![cfg(unix)]
use bfsort::{
    app::config::SecurityMode,
    repository::{
        metadata::{self, Occurrence},
        storage::Repository,
    },
};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bfsort"))
        .args(args)
        .output()
        .unwrap()
}
fn success(args: &[&str]) -> serde_json::Value {
    let result = cli(args);
    assert!(
        result.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}
fn ingest(directory: &Path) -> PathBuf {
    let inputs = directory.join("inputs");
    fs::create_dir(&inputs).unwrap();
    fs::write(
        inputs.join("message.txt"),
        b"catalog payload sentinel text\n",
    )
    .unwrap();
    let mut elf = vec![0u8; 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    elf[16] = 2;
    elf[18] = 62;
    elf[20] = 1;
    elf[52] = 64;
    fs::write(inputs.join("program.pdf"), elf).unwrap();
    let root = directory.join("repo");
    success(&[
        "ingest",
        text(&inputs),
        "--recursive",
        "true",
        "--security-mode",
        "standard",
        "-o",
        text(&root),
        "--json",
    ]);
    root
}
fn count(root: &Path, filters: &[&str]) -> usize {
    let mut args = vec!["query", text(root), "--count", "--json"];
    args.extend_from_slice(filters);
    success(&args)["occurrences"].as_u64().unwrap() as usize
}
fn private_write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn synthetic(index: usize) -> Occurrence {
    serde_json::from_value(serde_json::json!({
        "occurrence_id": metadata::digest(index.to_string().as_bytes()), "sha256": metadata::digest(b"abc"),
        "size":3,"file_type":"text","original_name":"a.txt","original_entry_name":"a.txt",
        "original_stem":"a","original_extension":"txt","source_archive":"","source_path":"legacy",
        "entry_index":index,"encrypted":false,"ingest_date":"2026-11-05","run_id":"legacy"
    })).unwrap()
}
fn legacy(root: &Path) {
    let repo = Repository::open(root, SecurityMode::Standard).unwrap();
    let mut file = tempfile::NamedTempFile::new_in(repo.staging()).unwrap();
    file.write_all(b"abc").unwrap();
    repo.commit(file, &metadata::digest(b"abc"), 3).unwrap();
    let mut journal = tempfile::NamedTempFile::new_in(repo.root().join("manifests")).unwrap();
    metadata::append(journal.as_file_mut(), &synthetic(0)).unwrap();
    journal
        .persist(repo.root().join("manifests/legacy.jsonl"))
        .unwrap();
}

#[test]
fn ingest_catalog_is_private_metadata_and_ignores_stale_jsonl() {
    let tmp = tempfile::tempdir().unwrap();
    let root = ingest(tmp.path());
    let database = root.join("metadata.sqlite3");
    assert!(database.exists());
    assert_eq!(
        fs::metadata(&database).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let connection = rusqlite::Connection::open(&database).unwrap();
    let blobs: i64 = connection
        .query_row(
            "SELECT count(*) FROM pragma_table_info('objects') WHERE upper(type) = 'BLOB'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(blobs, 0, "payloads must remain in object files");
    drop(connection);
    let rows = metadata::load(&root).unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert!(
            root.join("objects")
                .join(&row.sha256[..2])
                .join(row.sha256)
                .is_file()
        );
    }
    for entry in fs::read_dir(root.join("manifests")).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            fs::remove_file(path).unwrap();
        }
    }
    private_write(
        &root.join("manifests/stale.jsonl"),
        b"malformed stale export\n",
    );
    assert_eq!(count(&root, &[]), 2);
    assert_eq!(
        count(&root, &["--include-ext", "pdf", "--include-format", "elf"]),
        1
    );
    assert_eq!(
        count(&root, &["--include-ext", "pdf", "--include-format", "pdf"]),
        0
    );
    assert_eq!(
        count(
            &root,
            &["--include-ext", "pdf,txt", "--exclude-format", "elf"]
        ),
        1
    );
    assert_eq!(count(&root, &["--include-ext", "txt'"]), 0);
    assert_eq!(count(&root, &["--mismatch", "mismatch"]), 1);
    assert_eq!(
        count(
            &root,
            &[
                "--min-size",
                "60",
                "--max-size",
                "80",
                "--include-format",
                "elf"
            ]
        ),
        1
    );
    assert_eq!(count(&root, &["--min-size", "999999"]), 0);
    assert_eq!(count(&root, &["--max-size", "1"]), 0);
    success(&["verify", text(&root), "--full", "--json"]);
    success(&["prune", text(&root), "--dry-run", "--json"]);
}

#[test]
fn export_import_roundtrip_and_no_clobber() {
    let tmp = tempfile::tempdir().unwrap();
    let root = ingest(tmp.path());
    let exported = tmp.path().join("exchange.jsonl");
    success(&[
        "metadata",
        "export",
        text(&root),
        "-o",
        text(&exported),
        "--json",
    ]);
    assert!(
        !cli(&["metadata", "export", text(&root), "-o", text(&exported)])
            .status
            .success()
    );
    let destination = tmp.path().join("imported");
    {
        let repo = Repository::open(&destination, SecurityMode::Standard).unwrap();
        for row in metadata::load(&root).unwrap() {
            let bytes = fs::read(
                root.join("objects")
                    .join(&row.sha256[..2])
                    .join(&row.sha256),
            )
            .unwrap();
            let mut file = tempfile::NamedTempFile::new_in(repo.staging()).unwrap();
            file.write_all(&bytes).unwrap();
            repo.commit(file, &row.sha256, row.size).unwrap();
        }
    }
    success(&[
        "metadata",
        "import",
        text(&destination),
        text(&exported),
        "--json",
    ]);
    assert_eq!(count(&destination, &[]), 2);
    assert_eq!(count(&destination, &["--include-format", "elf"]), 1);
    assert_eq!(
        serde_json::to_value(metadata::load(&root).unwrap()).unwrap(),
        serde_json::to_value(metadata::load(&destination).unwrap()).unwrap()
    );
}

#[test]
fn legacy_read_and_preview_do_not_create_catalog() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("legacy");
    legacy(&root);
    assert!(!root.join("metadata.sqlite3").exists());
    let before = tree_files(&root);
    assert_eq!(count(&root, &[]), 1);
    success(&[
        "sample",
        text(&root),
        "--count",
        "1",
        "-o",
        text(&tmp.path().join("preview")),
        "--dry-run",
        "--json",
    ]);
    assert!(!root.join("metadata.sqlite3").exists());
    assert!(!tmp.path().join("preview").exists());
    assert_eq!(tree_files(&root), before);
    success(&["metadata", "migrate", text(&root), "--json"]);
    assert!(root.join("metadata.sqlite3").exists());
    assert_eq!(count(&root, &[]), 1);
}

#[test]
fn unsupported_schema_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = ingest(tmp.path());
    let connection = rusqlite::Connection::open(root.join("metadata.sqlite3")).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    drop(connection);
    assert!(
        !cli(&["query", text(&root), "--count", "--json"])
            .status
            .success()
    );
}

#[test]
fn malformed_import_does_not_publish_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    legacy(&root);
    // Migrate the existing row before checking transactional import rollback.
    let initial = tmp.path().join("initial.jsonl");
    let mut bytes = serde_json::to_vec(&synthetic(0)).unwrap();
    bytes.push(b'\n');
    private_write(&initial, &bytes);
    success(&["metadata", "import", text(&root), text(&initial), "--json"]);
    let invalid = tmp.path().join("invalid.jsonl");
    let mut bytes = serde_json::to_vec(&synthetic(1)).unwrap();
    bytes.extend_from_slice(b"\n{malformed}\n");
    private_write(&invalid, &bytes);
    assert!(
        !cli(&["metadata", "import", text(&root), text(&invalid), "--json"])
            .status
            .success()
    );
    assert_eq!(count(&root, &[]), 1);
}

#[test]
fn tombstones_reclassification_and_interchange_agree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = ingest(tmp.path());
    success(&[
        "remove",
        text(&root),
        "--include-ext",
        "pdf",
        "--confirm",
        "--json",
    ]);
    assert_eq!(count(&root, &[]), 1);
    success(&["reclassify", text(&root), "--json"]);
    assert_eq!(count(&root, &["--include-ext", "pdf"]), 0);
    let exported = tmp.path().join("active.jsonl");
    success(&[
        "metadata",
        "export",
        text(&root),
        "-o",
        text(&exported),
        "--json",
    ]);
    assert_eq!(fs::read_to_string(exported).unwrap().lines().count(), 1);
    success(&["verify", text(&root), "--full", "--json"]);
    success(&["prune", text(&root), "--dry-run", "--json"]);
}

#[test]
fn import_rejects_missing_objects_and_conflicting_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    legacy(&root);
    let input = tmp.path().join("conflict.jsonl");
    let mut conflict = synthetic(0);
    conflict.source_path = "conflicting provenance".to_owned();
    let mut bytes = serde_json::to_vec(&conflict).unwrap();
    bytes.push(b'\n');
    private_write(&input, &bytes);
    assert!(
        !cli(&["metadata", "import", text(&root), text(&input), "--json"])
            .status
            .success()
    );
    assert_eq!(metadata::load(&root).unwrap()[0].source_path, "legacy");
    let mut missing = synthetic(1);
    missing.sha256 = metadata::digest(b"absent bytes");
    let mut bytes = serde_json::to_vec(&missing).unwrap();
    bytes.push(b'\n');
    private_write(&input, &bytes);
    assert!(
        !cli(&["metadata", "import", text(&root), text(&input), "--json"])
            .status
            .success()
    );
    assert_eq!(count(&root, &[]), 1);
}

fn tree_files(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}
