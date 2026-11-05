#![cfg(unix)]
use bfsort::{
    app::config::SecurityMode,
    repository::{
        compare,
        metadata::{self, Occurrence},
        security,
        storage::Repository,
        verify,
    },
};
use std::{fs, io::Write, sync::atomic::AtomicBool};

fn fixture(root: &std::path::Path, mode: SecurityMode, copies: usize) -> String {
    let repo = Repository::open(root, mode).unwrap();
    let hash = metadata::digest(b"abc");
    let mut file = tempfile::NamedTempFile::new_in(repo.staging()).unwrap();
    file.write_all(b"abc").unwrap();
    repo.commit(file, &hash, 3).unwrap();
    let mut manifest = fs::File::create(root.join("manifests/run.jsonl")).unwrap();
    security::seal(&manifest, SecurityMode::Standard).unwrap();
    for index in 0..copies {
        let row: Occurrence = serde_json::from_value(serde_json::json!({
            "occurrence_id": metadata::digest(index.to_string().as_bytes()), "sha256":hash,
            "size":3,"file_type":"text","original_name":"a.txt","original_entry_name":"a.txt",
            "original_stem":"a","original_extension":"txt","source_archive":"source",
            "source_path":"source","entry_index":index,"encrypted":false,"ingest_date":"today","run_id":"run"
        })).unwrap();
        metadata::append(&mut manifest, &row).unwrap();
    }
    hash
}
#[test]
fn verifies_unique_objects_and_detects_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let hash = fixture(&root, SecurityMode::Standard, 2);
    let report = verify::run(&root, true, &AtomicBool::new(false)).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.verified, 1);
    fs::write(root.join("objects").join(&hash[..2]).join(&hash), b"xyz").unwrap();
    assert!(
        !verify::run(&root, true, &AtomicBool::new(false))
            .unwrap()
            .failures
            .is_empty()
    );
}
#[test]
fn sealed_full_is_refused_without_changing_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let hash = fixture(&root, SecurityMode::Sealed, 1);
    assert_eq!(
        verify::run(&root, false, &AtomicBool::new(false))
            .unwrap()
            .metadata_only,
        1
    );
    assert!(verify::run(&root, true, &AtomicBool::new(false)).is_err());
    assert_eq!(
        fs::metadata(root.join("objects").join(&hash[..2]).join(hash))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0
    );
}
#[test]
fn compares_unique_hashes_and_occurrence_counts() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    fixture(&left, SecurityMode::Sealed, 1);
    fixture(&right, SecurityMode::Sealed, 2);
    let report = compare::run(&left, &right).unwrap();
    assert!(report.added.is_empty() && report.removed.is_empty());
    assert_eq!(report.occurrence_counts.len(), 1);
    assert!(
        compare::run(&left, &left)
            .unwrap()
            .occurrence_counts
            .is_empty()
    );
}

#[test]
fn missing_truncated_and_unsafe_objects_fail_metadata_verification() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let hash = fixture(&root, SecurityMode::Standard, 1);
    let object = root.join("objects").join(&hash[..2]).join(&hash);
    fs::write(&object, b"x").unwrap();
    assert_eq!(
        verify::run(&root, false, &AtomicBool::new(false))
            .unwrap()
            .corrupt,
        1
    );
    fs::remove_file(&object).unwrap();
    assert_eq!(
        verify::run(&root, false, &AtomicBool::new(false))
            .unwrap()
            .missing,
        1
    );
    std::os::unix::fs::symlink(root.join("security.json"), &object).unwrap();
    assert_eq!(
        verify::run(&root, false, &AtomicBool::new(false))
            .unwrap()
            .corrupt,
        1
    );
}

#[test]
fn interrupted_manifest_tail_is_tolerated_and_cancellation_preserves_staging() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    fixture(&root, SecurityMode::Standard, 1);
    let mut manifest = fs::OpenOptions::new()
        .append(true)
        .open(root.join("manifests/run.jsonl"))
        .unwrap();
    manifest.write_all(b"{interrupted").unwrap();
    let stage = root.join(".staging/run-unfinished");
    fs::create_dir(&stage).unwrap();
    assert!(
        verify::run(&root, false, &AtomicBool::new(false))
            .unwrap()
            .failures
            .is_empty()
    );
    assert!(stage.exists());
    assert!(verify::run(&root, true, &AtomicBool::new(true)).is_err());
    assert!(stage.exists());
}

#[test]
fn unknown_classification_reference_and_unsafe_current_fail() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    fixture(&root, SecurityMode::Standard, 1);
    let pointer = root.join("views/CURRENT");
    fs::write(&pointer, "../objects").unwrap();
    security::seal(&fs::File::open(&pointer).unwrap(), SecurityMode::Standard).unwrap();
    assert!(
        !verify::run(&root, false, &AtomicBool::new(false))
            .unwrap()
            .failures
            .is_empty()
    );
    fs::remove_file(pointer).unwrap();
    let mut row: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("manifests/run.jsonl")).unwrap()).unwrap();
    row["occurrence_id"] = serde_json::json!("f".repeat(64));
    let overlay = root.join("reports/classification.jsonl");
    fs::write(&overlay, format!("{}\n", row)).unwrap();
    security::seal(&fs::File::open(overlay).unwrap(), SecurityMode::Standard).unwrap();
    assert!(
        !verify::run(&root, false, &AtomicBool::new(false))
            .unwrap()
            .failures
            .is_empty()
    );
}

#[test]
fn diff_uses_effective_detection_and_ignores_unrelated_provenance() {
    let temp = tempfile::tempdir().unwrap();
    let left = temp.path().join("left");
    let right = temp.path().join("right");
    fixture(&left, SecurityMode::Sealed, 1);
    fixture(&right, SecurityMode::Sealed, 1);
    let mut row = metadata::load(&right).unwrap().remove(0);
    row.occurrence_id = metadata::digest(b"unrelated occurrence");
    row.source_path = "different source".into();
    fs::write(
        right.join("manifests/run.jsonl"),
        format!("{}\n", serde_json::to_string(&row).unwrap()),
    )
    .unwrap();
    let report = compare::run(&left, &right).unwrap();
    assert!(report.detection_differences.is_empty());
    assert!(report.occurrence_counts.is_empty());
    row.detection.detector_version = "alternate-detector".into();
    let path = right.join("reports/classification.jsonl");
    fs::write(&path, format!("{}\n", serde_json::to_string(&row).unwrap())).unwrap();
    security::seal(&fs::File::open(path).unwrap(), SecurityMode::Standard).unwrap();
    let report = compare::run(&left, &right).unwrap();
    assert_eq!(report.detection_differences.len(), 1);
    assert_eq!(report.right_detector_versions, ["alternate-detector"]);
}
