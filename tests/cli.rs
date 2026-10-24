use std::process::Command;

#[test]
fn help_and_version_are_available_without_starting_the_ui() {
    let binary = env!("CARGO_BIN_EXE_sozo");
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("Usage: sozo [DIRECTORY]"));
    assert!(help.contains("Defaults to the current directory"));

    let version = Command::new(binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("sozo {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rejects_a_missing_directory() {
    let output = Command::new(env!("CARGO_BIN_EXE_sozo"))
        .arg("a-directory-that-does-not-exist-sozo")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Could not open directory"));
}
