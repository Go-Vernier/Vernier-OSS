use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_blast-radius"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name)
}

#[test]
fn analyze_json_prints_the_contract() {
    let out = bin()
        .args([
            "analyze",
            fixture("compose-app").to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["discovery"]["strategy"], "docker-compose");
    assert_eq!(json["services"].as_array().unwrap().len(), 3);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("{\n  \"repository\""),
        "pretty printed with two spaces"
    );
}

#[test]
fn analyze_report_defaults_to_the_current_directory_and_has_no_colour_when_piped() {
    let out = bin()
        .arg("analyze")
        .current_dir(fixture("monorepo-app"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("3 detected  (monorepo)"), "{text}");
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn errors_go_to_stderr_with_exit_code_1() {
    let out = bin()
        .args([
            "analyze",
            fixture("compose-app")
                .join("docker-compose.yml")
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("blast-radius: not a directory"), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn version_flag() {
    let out = bin().arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("blast-radius 0.0.1"));
}
