#[cfg(not(unix))]
#[test]
/// Ensure unsupported platforms fail closed instead of weakening storage.
fn storage_fails_closed_without_acl_backend() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    assert!(
        malware_organizer::storage::Repository::open(
            &root,
            malware_organizer::config::SecurityMode::Sealed
        )
        .is_err()
    );
    assert!(!root.exists());
}
#[test]
/// Ensure shipped example configuration files remain parseable.
fn example_configuration_and_rules_are_valid() {
    let config: malware_organizer::config::Config =
        toml::from_str(include_str!("../config.example.toml")).unwrap();
    config.validate().unwrap();
    malware_organizer::rules::Rules::parse(include_str!("../rules.example.toml")).unwrap();
}
