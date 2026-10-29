use std::process::Command;

#[test]
/// Ensure help exposes every top-level command.
fn help_lists_commands() {
    let out = Command::new(env!("CARGO_BIN_EXE_malware-organizer"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for command in ["ingest", "inspect", "stats", "reorganize"] {
        assert!(help.contains(command), "missing {command}");
    }
}

#[test]
/// Ensure invalid configuration fails before creating the output path.
fn rejects_invalid_configuration_before_output_creation() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "workers = 0\n").unwrap();
    let output = dir.path().join("repo");
    let status = Command::new(env!("CARGO_BIN_EXE_malware-organizer"))
        .args(["ingest", "--config"])
        .arg(config)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!status.status.success());
    assert!(!output.exists());
}
