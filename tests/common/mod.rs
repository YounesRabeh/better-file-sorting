#![allow(dead_code)]
use std::{
    fs::File,
    io::Write,
    path::Path,
    process::{Command, Output},
};
/// Create a synthetic ZIP archive for integration tests.
pub fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}
/// Run the compiled CLI with native argument values.
pub fn run(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_malware-organizer"))
        .args(args)
        .output()
        .unwrap()
}
/// Run ingestion with a selected worker count.
pub fn ingest(input: &Path, output: &Path, workers: &str) -> Output {
    run(&[
        "ingest".as_ref(),
        input.as_os_str(),
        "--output".as_ref(),
        output.as_os_str(),
        "--workers".as_ref(),
        workers.as_ref(),
    ])
}
/// Read all JSONL occurrence records from a test repository.
pub fn manifest(root: &Path) -> Vec<serde_json::Value> {
    let mut result = vec![];
    for entry in std::fs::read_dir(root.join("manifests")).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in text.lines() {
            result.push(serde_json::from_str(line).unwrap());
        }
    }
    result
}
