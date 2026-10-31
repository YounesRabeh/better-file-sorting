//! Untrusted inputs harness for BFS.
//! Uses synthetic inputs to exercise the library independently of normal CLI usage.
//! Keep harness work isolated from user payloads and production repositories.

#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 { return; }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = bfs::rules::Rules::parse(text);
        let _ = bfs::extraction::validate_name(text);
        let _ = bfs::rules::sanitize(text);
    }
    let _ = bfs::classifier::classify(data);
    if let Ok(archive) = zip::ZipArchive::new(std::io::Cursor::new(data)) {
        for index in 0..archive.len().min(1000) {
            if let Ok(entry) = archive.by_index_data(index) { let _ = entry.name(); }
        }
    }
});
