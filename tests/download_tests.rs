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
        stream.read(&mut request).unwrap();
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
