#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 { return; }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = malware_organizer::rules::Rules::parse(text);
        let _ = malware_organizer::extraction::validate_name(text);
        let _ = malware_organizer::rules::sanitize(text);
    }
    let _ = malware_organizer::classifier::classify(data);
    if let Ok(archive) = zip::ZipArchive::new(std::io::Cursor::new(data)) {
        for index in 0..archive.len().min(1000) {
            if let Ok(entry) = archive.by_index_data(index) { let _ = entry.name(); }
        }
    }
});
