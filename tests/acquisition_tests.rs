use bfs::acquisition::parse_bytes;

#[test]
fn readable_sizes() {
    assert_eq!(parse_bytes("1 GB").unwrap(), 1_000_000_000);
    assert_eq!(parse_bytes("1 GiB").unwrap(), 1_073_741_824);
    for invalid in ["-1 MB", "1.5 MB", "12", "0 B", "18446744073709551615 TB"] {
        assert!(parse_bytes(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn policies_exclude_before_include_and_sort_titles() {
    use bfs::acquisition::*;
    let config: DownloadConfig = toml::from_str(
        r#"
        [[policies]]
        title_contains = "linux"
        action = "include"
        [[policies]]
        title_contains = "old"
        action = "exclude"
    "#,
    )
    .unwrap();
    let inventory = Inventory {
        version: 1,
        candidates: ["Linux Z", "Windows", "OLD Linux", "linux A"]
            .into_iter()
            .map(|title| Candidate {
                source_id: title.into(),
                url: String::new(),
                title: title.into(),
                name: String::new(),
                discovered_at: String::new(),
            })
            .collect(),
    };
    let selected = select_candidates(&inventory, &config).unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|c| c.title.as_str())
            .collect::<Vec<_>>(),
        ["linux A", "Linux Z"]
    );
}

#[test]
fn limits_and_directory_validation() {
    use bfs::acquisition::DownloadConfig;
    for setting in [
        "max_speed = '0 MB/s'",
        "max_speed = '2 MB'",
        "max_requests = 0",
        "directory = '../escape'",
        "concurrency = 0",
    ] {
        assert!(
            toml::from_str::<DownloadConfig>(setting)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let config: DownloadConfig =
        toml::from_str("max_speed = '2 MB/s'\nmin_request_interval_ms = 0").unwrap();
    assert_eq!(config.speed().unwrap(), Some(2_000_000));
    assert_eq!(DownloadConfig::default().speed().unwrap(), None);
}

#[test]
fn existing_staging_directory_must_be_private() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("payloads");
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(bfs::acquisition::private_tree(&path).is_err());
}
